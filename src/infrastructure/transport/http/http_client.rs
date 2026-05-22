use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use super::http_error_from_response_status;
use crate::error::{FlareError, Result};
use tokio::sync::RwLock;
use uuid::Uuid;

/// 从 Bearer JWT payload 提取 `sub`（不校验签名，仅用于补齐 `x-user-id` header）。
fn jwt_sub_unverified(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?.trim();
    if payload.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(payload))
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("sub")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[derive(Default)]
pub struct HttpRequestContext {
    /// IM 长连接 / 媒体等 core HTTP 使用的 Bearer
    auth_token: RwLock<String>,
    /// Social Gateway 等业务 BFF 使用的 Bearer（与 IM token 分离，避免建连时互相覆盖）
    gateway_token: RwLock<String>,
    tenant_id: RwLock<String>,
    user_id: RwLock<String>,
    trace_id: RwLock<String>,
    language: RwLock<String>,
}

impl HttpRequestContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn set_auth_context(&self, token: String, language: Option<String>) {
        *self.auth_token.write().await = token;
        if let Some(language) = language {
            *self.language.write().await = language;
        }
    }

    /// Gateway / BFF 会话：Bearer + `x-tenant-id` / `x-user-id` / `x-trace-id`
    pub async fn set_gateway_context(
        &self,
        token: String,
        tenant_id: String,
        user_id: String,
        language: Option<String>,
    ) {
        *self.gateway_token.write().await = token;
        *self.tenant_id.write().await = tenant_id;
        *self.user_id.write().await = user_id;
        if self.trace_id.read().await.is_empty() {
            *self.trace_id.write().await = Uuid::new_v4().to_string();
        }
        if let Some(language) = language {
            *self.language.write().await = language;
        }
    }

    pub async fn clear_gateway_context(&self) {
        *self.gateway_token.write().await = String::new();
        *self.user_id.write().await = String::new();
    }

    pub async fn gateway_token_is_empty(&self) -> bool {
        self.gateway_token.read().await.trim().is_empty()
    }

    /// 补齐或刷新 `x-user-id` / `x-tenant-id`（IM `login` 后 Background 社交同步依赖此字段）。
    pub async fn ensure_identity(&self, user_id: &str, tenant_id: &str) {
        let uid = user_id.trim();
        if !uid.is_empty() {
            *self.user_id.write().await = uid.to_string();
        }
        let tid = tenant_id.trim();
        if !tid.is_empty() {
            *self.tenant_id.write().await = tid.to_string();
        }
    }

    /// 从当前 Gateway/IM Bearer 解析 `sub` 并写回 `user_id`（JWT 形态：`a.b.c`）。
    pub async fn sync_user_id_from_token(&self) {
        let gateway_token = self.gateway_token.read().await.clone();
        let im_token = self.auth_token.read().await.clone();
        let token = if !gateway_token.trim().is_empty() {
            gateway_token
        } else {
            im_token
        };
        if let Some(sub) = jwt_sub_unverified(token.trim()) {
            *self.user_id.write().await = sub;
        }
    }

    pub async fn set_trace_id(&self, trace_id: String) {
        *self.trace_id.write().await = trace_id;
    }

    pub async fn build_headers(&self) -> HashMap<String, String> {
        let gateway_token = self.gateway_token.read().await.clone();
        let im_token = self.auth_token.read().await.clone();
        let token = if !gateway_token.trim().is_empty() {
            gateway_token
        } else {
            im_token
        };
        let mut tenant_id = self.tenant_id.read().await.trim().to_string();
        let mut user_id = self.user_id.read().await.trim().to_string();
        if user_id.is_empty() && !token.trim().is_empty() {
            if let Some(sub) = jwt_sub_unverified(token.trim()) {
                user_id = sub.clone();
                *self.user_id.write().await = sub;
            }
        }
        if tenant_id.is_empty() {
            tenant_id = "0".to_string();
            *self.tenant_id.write().await = tenant_id.clone();
        }
        let trace_id = self.trace_id.read().await.clone();
        let language = self.language.read().await.clone();
        let mut headers = HashMap::new();
        if !token.trim().is_empty() {
            headers.insert(
                "Authorization".to_string(),
                format!("Bearer {}", token.trim()),
            );
        }
        if !tenant_id.is_empty() {
            headers.insert("x-tenant-id".to_string(), tenant_id);
        }
        if !user_id.is_empty() {
            headers.insert("x-user-id".to_string(), user_id);
        }
        if trace_id.is_empty() {
            headers.insert("x-trace-id".to_string(), Uuid::new_v4().to_string());
        } else {
            headers.insert("x-trace-id".to_string(), trace_id);
        }
        headers.insert("x-request-id".to_string(), Uuid::new_v4().to_string());
        if !language.trim().is_empty() {
            headers.insert("Accept-Language".to_string(), language);
        }
        headers
    }
}

#[derive(Clone)]
pub struct HttpClient {
    base_url: String,
    context: Option<Arc<HttpRequestContext>>,
    #[cfg(not(target_arch = "wasm32"))]
    client: reqwest::Client,
}

impl HttpClient {
    #[cfg(not(target_arch = "wasm32"))]
    fn build_reqwest_client() -> reqwest::Client {
        const HTTP_CONNECT_TIMEOUT_SECS: u64 = 5;
        const HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
        match reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!("build reqwest client with timeout failed: {err}");
                reqwest::Client::new()
            }
        }
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            context: None,
            #[cfg(not(target_arch = "wasm32"))]
            client: Self::build_reqwest_client(),
        }
    }

    pub fn with_context(base_url: impl Into<String>, context: Arc<HttpRequestContext>) -> Self {
        Self {
            base_url: base_url.into(),
            context: Some(context),
            #[cfg(not(target_arch = "wasm32"))]
            client: Self::build_reqwest_client(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 发 Social Gateway 请求前补齐身份（先写显式 user/tenant，再从 JWT 回填）。
    pub async fn ensure_request_identity(&self, user_id: &str, tenant_id: &str) {
        if let Some(ctx) = &self.context {
            ctx.ensure_identity(user_id, tenant_id).await;
            ctx.sync_user_id_from_token().await;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get<T>(&self, path: &str, query: Option<&HashMap<String, String>>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request(
            reqwest::Method::GET,
            self.build_url(path),
            query,
            None::<&()>,
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_with_headers<T>(
        &self,
        path: &str,
        query: Option<&HashMap<String, String>>,
        headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request_with_headers(
            reqwest::Method::GET,
            self.build_url(path),
            query,
            None::<&()>,
            headers,
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_full_url<T>(
        &self,
        url: &str,
        query: Option<&HashMap<String, String>>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::GET, url.to_string(), query, None::<&()>)
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn post<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request(
            reqwest::Method::POST,
            self.build_url(path),
            None,
            Some(body),
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn post_with_headers<B, T>(
        &self,
        path: &str,
        body: &B,
        headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request_with_headers(
            reqwest::Method::POST,
            self.build_url(path),
            None,
            Some(body),
            headers,
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn post_full_url<B, T>(&self, url: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::POST, url.to_string(), None, Some(body))
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn put<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::PUT, self.build_url(path), None, Some(body))
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn put_full_url<B, T>(&self, url: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::PUT, url.to_string(), None, Some(body))
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn put_bytes_full_url(
        &self,
        url: &str,
        data: &[u8],
        headers: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut req = self.client.put(url.to_string()).body(data.to_vec());
        if let Some(context) = &self.context {
            for (key, value) in context.build_headers().await {
                req = req.header(key, value);
            }
        }
        for (key, value) in headers {
            req = req.header(key, value);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http put bytes failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FlareError::general_error(format!(
                "http put bytes status not success: {status}"
            )));
        }
        let mut out = HashMap::new();
        for (key, value) in resp.headers() {
            if let Ok(text) = value.to_str() {
                out.insert(key.as_str().to_string(), text.to_string());
            }
        }
        Ok(out)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn delete<T>(&self, path: &str, query: Option<&HashMap<String, String>>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request(
            reqwest::Method::DELETE,
            self.build_url(path),
            query,
            None::<&()>,
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn delete_full_url<T>(
        &self,
        url: &str,
        query: Option<&HashMap<String, String>>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::DELETE, url.to_string(), query, None::<&()>)
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn delete_with_body<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request(
            reqwest::Method::DELETE,
            self.build_url(path),
            None,
            Some(body),
        )
        .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn delete_with_body_headers<B, T>(
        &self,
        path: &str,
        body: &B,
        headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        self.request_with_headers(
            reqwest::Method::DELETE,
            self.build_url(path),
            None,
            Some(body),
            headers,
        )
        .await
    }

    /// GET 任意 http(s) URL，返回原始字节（不附加鉴权头，适用于 S3/RustFS 等预签名直链）。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_bytes_direct_url(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http get bytes failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FlareError::general_error(format!(
                "http get bytes status not success: {status}"
            )));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| FlareError::system(format!("http read body failed: {e}")))
    }

    /// GET 任意 http(s) URL，返回成功响应体流（不附加鉴权头，适用于预签名直链）；调用方负责消费 `bytes_stream`。
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get_response_direct_url(&self, url: &str) -> Result<reqwest::Response> {
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http get stream failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FlareError::general_error(format!(
                "http get stream status not success: {status}"
            )));
        }
        Ok(resp)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn send_bytes(&self, path: &str, data: &[u8]) -> Result<Vec<u8>> {
        let url = self.build_url(path);
        let resp = self
            .client
            .post(url)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http send_bytes failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FlareError::general_error(format!(
                "http send_bytes status={status}"
            )));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| FlareError::system(format!("http read bytes failed: {e}")))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn send_bytes(&self, _path: &str, _data: &[u8]) -> Result<Vec<u8>> {
        Err(FlareError::system(
            "HTTP transport not supported in wasm build",
        ))
    }

    fn build_url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_string();
        }
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn request<B, T>(
        &self,
        method: reqwest::Method,
        url: String,
        query: Option<&HashMap<String, String>>,
        body: Option<&B>,
    ) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let mut req = self.client.request(method, url);
        if let Some(context) = &self.context {
            for (key, value) in context.build_headers().await {
                req = req.header(key, value);
            }
        }
        if let Some(q) = query {
            req = req.query(q);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error_from_response_status(status.as_u16(), &body));
        }
        resp.json()
            .await
            .map_err(|e| FlareError::system(format!("parse http json failed: {e}")))
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn request_with_headers<B, T>(
        &self,
        method: reqwest::Method,
        url: String,
        query: Option<&HashMap<String, String>>,
        body: Option<&B>,
        headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let mut req = self.client.request(method, url);
        if let Some(context) = &self.context {
            for (key, value) in context.build_headers().await {
                req = req.header(key, value);
            }
        }
        if let Some(q) = query {
            req = req.query(q);
        }
        for (key, value) in headers {
            req = req.header(key, value);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(http_error_from_response_status(status.as_u16(), &body));
        }
        resp.json()
            .await
            .map_err(|e| FlareError::system(format!("parse http json failed: {e}")))
    }
}
