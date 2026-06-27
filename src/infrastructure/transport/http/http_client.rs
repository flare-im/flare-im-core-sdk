use std::collections::HashMap;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use super::http_client_wasm as wasm_http;
#[cfg(not(target_arch = "wasm32"))]
use super::http_error_from_response_status;
use crate::shared::error::{FlareError, Result};
use base64::Engine as _;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::common::TlsConfig as CoreTlsConfig;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::common::cert::create_client_config_with_tls;
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
        *self.tenant_id.write().await = String::new();
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
            *self.tenant_id.write().await = crate::shared::util::normalize_tenant_id(tid);
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
        let mut tenant_id =
            crate::shared::util::normalize_tenant_id(self.tenant_id.read().await.trim());
        let mut user_id = self.user_id.read().await.trim().to_string();
        if user_id.is_empty()
            && !token.trim().is_empty()
            && let Some(sub) = jwt_sub_unverified(token.trim())
        {
            user_id = sub.clone();
            *self.user_id.write().await = sub;
        }
        if tenant_id.is_empty() {
            tenant_id = "0".to_string();
        }
        *self.tenant_id.write().await = tenant_id.clone();
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

#[cfg(test)]
mod tests {
    use super::HttpRequestContext;

    #[tokio::test]
    async fn clear_gateway_context_falls_back_to_im_auth_without_stale_identity() {
        let context = HttpRequestContext::new();
        context.set_auth_context("im-token".to_string(), None).await;
        context
            .set_gateway_context(
                "old-gateway-token".to_string(),
                "tenant-a".to_string(),
                "alice".to_string(),
                None,
            )
            .await;

        let headers = context.build_headers().await;
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer old-gateway-token")
        );
        assert_eq!(
            headers.get("x-tenant-id").map(String::as_str),
            Some("tenant-a")
        );
        assert_eq!(headers.get("x-user-id").map(String::as_str), Some("alice"));

        context.clear_gateway_context().await;

        let headers = context.build_headers().await;
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer im-token")
        );
        assert_eq!(headers.get("x-tenant-id").map(String::as_str), Some("0"));
        assert_eq!(headers.get("x-user-id"), None);
    }
}

#[derive(Clone)]
pub struct HttpClient {
    base_url: String,
    context: Option<Arc<HttpRequestContext>>,
    direct_url_rewrite_prefix: Option<String>,
    direct_url_rewrite_targets: Vec<String>,
    #[cfg(not(target_arch = "wasm32"))]
    client: reqwest::Client,
    #[cfg(not(target_arch = "wasm32"))]
    tls_config_error: Option<String>,
}

impl HttpClient {
    #[cfg(not(target_arch = "wasm32"))]
    fn build_reqwest_client(tls: Option<&CoreTlsConfig>) -> (reqwest::Client, Option<String>) {
        const HTTP_CONNECT_TIMEOUT_SECS: u64 = 5;
        const HTTP_REQUEST_TIMEOUT_SECS: u64 = 30;
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
            .timeout(Duration::from_secs(HTTP_REQUEST_TIMEOUT_SECS));
        let mut tls_config_error = None;
        if let Some(tls) = tls
            && tls.requires_custom_client_tls()
        {
            match create_client_config_with_tls(tls) {
                Ok(config) => builder = builder.use_preconfigured_tls(config),
                Err(err) => {
                    let message = format!("build reqwest pinned TLS config failed: {err}");
                    tracing::warn!("{message}");
                    tls_config_error = Some(message);
                }
            }
        }
        let client = match builder.build() {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!("build reqwest client with timeout failed: {err}");
                reqwest::Client::new()
            }
        };
        (client, tls_config_error)
    }

    pub fn new(base_url: impl Into<String>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (client, tls_config_error) = Self::build_reqwest_client(None);
        Self {
            base_url: base_url.into(),
            context: None,
            direct_url_rewrite_prefix: None,
            direct_url_rewrite_targets: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            client,
            #[cfg(not(target_arch = "wasm32"))]
            tls_config_error,
        }
    }

    pub fn with_context(base_url: impl Into<String>, context: Arc<HttpRequestContext>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let (client, tls_config_error) = Self::build_reqwest_client(None);
        Self {
            base_url: base_url.into(),
            context: Some(context),
            direct_url_rewrite_prefix: None,
            direct_url_rewrite_targets: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            client,
            #[cfg(not(target_arch = "wasm32"))]
            tls_config_error,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_context_and_tls(
        base_url: impl Into<String>,
        context: Arc<HttpRequestContext>,
        tls: CoreTlsConfig,
    ) -> Self {
        let (client, tls_config_error) = Self::build_reqwest_client(Some(&tls));
        Self {
            base_url: base_url.into(),
            context: Some(context),
            direct_url_rewrite_prefix: None,
            direct_url_rewrite_targets: Vec::new(),
            client,
            tls_config_error,
        }
    }

    pub fn with_direct_url_rewrite(mut self, prefix: Option<String>, targets: Vec<String>) -> Self {
        self.direct_url_rewrite_prefix = prefix
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        self.direct_url_rewrite_targets = targets
            .into_iter()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_tls_ready(&self) -> Result<()> {
        if let Some(error) = &self.tls_config_error {
            return Err(FlareError::localized(
                crate::shared::error::ErrorCode::ConfigurationError,
                error.clone(),
            ));
        }
        Ok(())
    }

    /// 发 Social Gateway 请求前补齐身份（先写显式 user/tenant，再从 JWT 回填）。
    pub async fn ensure_request_identity(&self, user_id: &str, tenant_id: &str) {
        if let Some(ctx) = &self.context {
            ctx.ensure_identity(user_id, tenant_id).await;
            ctx.sync_user_id_from_token().await;
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn context_headers(&self) -> HashMap<String, String> {
        if let Some(ctx) = &self.context {
            ctx.build_headers().await
        } else {
            HashMap::new()
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
        self.ensure_tls_ready()?;
        let mut req = self.client.put(url.to_string()).body(data.to_vec());
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
    pub async fn put_file_full_url(
        &self,
        url: &str,
        path: &std::path::Path,
        content_len: u64,
        headers: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        self.ensure_tls_ready()?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| FlareError::general_error(format!("open upload file failed: {e}")))?;
        let mut req = self
            .client
            .put(url.to_string())
            .body(reqwest::Body::from(file));
        for (key, value) in headers {
            if !key.eq_ignore_ascii_case(reqwest::header::CONTENT_LENGTH.as_str()) {
                req = req.header(key, value);
            }
        }
        req = req.header(reqwest::header::CONTENT_LENGTH, content_len.to_string());
        let resp = req
            .send()
            .await
            .map_err(|e| FlareError::system(format!("http put file failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(FlareError::general_error(format!(
                "http put file status not success: {status}"
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
    pub async fn send_bytes(&self, path: &str, data: &[u8]) -> Result<Vec<u8>> {
        let headers = self.context_headers().await;
        wasm_http::fetch_bytes(
            "POST",
            self.build_url(path),
            None,
            Some(data.to_vec()),
            Some("application/octet-stream"),
            headers,
            None,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn get<T>(&self, path: &str, query: Option<&HashMap<String, String>>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        wasm_http::fetch_json("GET", self.build_url(path), query, None, headers, None).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn get_with_headers<T>(
        &self,
        path: &str,
        query: Option<&HashMap<String, String>>,
        extra_headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        wasm_http::fetch_json(
            "GET",
            self.build_url(path),
            query,
            None,
            headers,
            Some(extra_headers),
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn get_full_url<T>(
        &self,
        url: &str,
        query: Option<&HashMap<String, String>>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        wasm_http::fetch_json("GET", url.to_string(), query, None, HashMap::new(), None).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn post<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "POST",
            self.build_url(path),
            None,
            Some(&body_json),
            headers,
            None,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn post_with_headers<B, T>(
        &self,
        path: &str,
        body: &B,
        extra_headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "POST",
            self.build_url(path),
            None,
            Some(&body_json),
            headers,
            Some(extra_headers),
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn post_full_url<B, T>(&self, url: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "POST",
            url.to_string(),
            None,
            Some(&body_json),
            HashMap::new(),
            None,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn put<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "PUT",
            self.build_url(path),
            None,
            Some(&body_json),
            headers,
            None,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn put_full_url<B, T>(&self, url: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "PUT",
            url.to_string(),
            None,
            Some(&body_json),
            HashMap::new(),
            None,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn put_bytes_full_url(
        &self,
        url: &str,
        data: &[u8],
        extra_headers: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let (_, headers) = wasm_http::fetch_bytes_with_headers(
            "PUT",
            self.rewrite_direct_url(url),
            None,
            Some(data.to_vec()),
            None,
            HashMap::new(),
            Some(extra_headers),
        )
        .await?;
        Ok(headers)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn delete<T>(&self, path: &str, query: Option<&HashMap<String, String>>) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        wasm_http::fetch_json("DELETE", self.build_url(path), query, None, headers, None).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn delete_full_url<T>(
        &self,
        url: &str,
        query: Option<&HashMap<String, String>>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        wasm_http::fetch_json("DELETE", url.to_string(), query, None, HashMap::new(), None).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn delete_with_body<B, T>(&self, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "DELETE",
            self.build_url(path),
            None,
            Some(&body_json),
            headers,
            None,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn delete_with_body_headers<B, T>(
        &self,
        path: &str,
        body: &B,
        extra_headers: &HashMap<String, String>,
    ) -> Result<T>
    where
        B: serde::Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let headers = self.context_headers().await;
        let body_json = serde_json::to_string(body)
            .map_err(|e| FlareError::system(format!("encode http json body failed: {e}")))?;
        wasm_http::fetch_json(
            "DELETE",
            self.build_url(path),
            None,
            Some(&body_json),
            headers,
            Some(extra_headers),
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn get_bytes_direct_url(&self, url: &str) -> Result<Vec<u8>> {
        wasm_http::fetch_bytes(
            "GET",
            self.rewrite_direct_url(url),
            None,
            None,
            None,
            HashMap::new(),
            None,
        )
        .await
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

    fn rewrite_direct_url(&self, url: &str) -> String {
        let Some(prefix) = self.direct_url_rewrite_prefix.as_deref() else {
            return url.to_string();
        };
        for target in &self.direct_url_rewrite_targets {
            if url == target {
                return prefix.to_string();
            }
            if let Some(rest) = url.strip_prefix(&format!("{target}/")) {
                return format!("{prefix}/{rest}");
            }
            if let Some(rest) = url.strip_prefix(&format!("{target}?")) {
                return format!("{prefix}?{rest}");
            }
        }
        url.to_string()
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
        self.ensure_tls_ready()?;
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
        self.ensure_tls_ready()?;
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
