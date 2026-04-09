use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{FlareError, Result};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct HttpRequestContext {
    auth_token: RwLock<String>,
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

    pub async fn build_headers(&self) -> HashMap<String, String> {
        let token = self.auth_token.read().await.clone();
        let language = self.language.read().await.clone();
        let mut headers = HashMap::new();
        if !token.trim().is_empty() {
            headers.insert(
                "Authorization".to_string(),
                format!("Bearer {}", token.trim()),
            );
        }
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

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn get<T>(
        &self,
        path: &str,
        query: Option<&HashMap<String, String>>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::GET, self.build_url(path), query, None::<&()>)
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
        self.request(reqwest::Method::POST, self.build_url(path), None, Some(body))
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
    pub async fn delete<T>(
        &self,
        path: &str,
        query: Option<&HashMap<String, String>>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request(reqwest::Method::DELETE, self.build_url(path), query, None::<&()>)
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
        Err(FlareError::system("HTTP transport not supported in wasm build"))
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
            return Err(FlareError::general_error(format!(
                "http status not success: {status}"
            )));
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
            return Err(FlareError::general_error(format!(
                "http status not success: {status}"
            )));
        }
        resp.json()
            .await
            .map_err(|e| FlareError::system(format!("parse http json failed: {e}")))
    }
}
