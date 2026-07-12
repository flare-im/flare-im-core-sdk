//! Browser HTTP transport via `fetch` (wasm32).

use std::collections::HashMap;

use js_sys::Uint8Array;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, RequestMode, Response};

use super::http_error_from_response_status;
use crate::shared::error::{FlareError, Result};

pub fn build_url(base_url: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

fn append_query(url: String, query: Option<&HashMap<String, String>>) -> String {
    let Some(query) = query else {
        return url;
    };
    if query.is_empty() {
        return url;
    }
    let mut parts = Vec::with_capacity(query.len());
    for (key, value) in query {
        parts.push(format!(
            "{}={}",
            js_sys::encode_uri_component(key),
            js_sys::encode_uri_component(value)
        ));
    }
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{}", parts.join("&"))
}

fn apply_headers(
    headers: &Headers,
    context_headers: &HashMap<String, String>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<()> {
    for (key, value) in context_headers {
        headers
            .set(key, value)
            .map_err(|e| FlareError::system(format!("http set header failed: {e:?}")))?;
    }
    if let Some(extra) = extra_headers {
        for (key, value) in extra {
            headers
                .set(key, value)
                .map_err(|e| FlareError::system(format!("http set header failed: {e:?}")))?;
        }
    }
    Ok(())
}

pub async fn fetch_bytes(
    method: &str,
    url: String,
    query: Option<&HashMap<String, String>>,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
    context_headers: HashMap<String, String>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<Vec<u8>> {
    let (bytes, _) = fetch_bytes_with_headers(
        method,
        url,
        query,
        body,
        content_type,
        context_headers,
        extra_headers,
    )
    .await?;
    Ok(bytes)
}

pub async fn fetch_bytes_with_headers(
    method: &str,
    url: String,
    query: Option<&HashMap<String, String>>,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
    context_headers: HashMap<String, String>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<(Vec<u8>, HashMap<String, String>)> {
    // 浏览器 `fetch` 全程持有 `JsFuture`/`web_sys` 句柄，其 future 本质 `!Send`。
    // 若让它直接冒泡到 SyncTask::execute，会连带把 IMClient（`Arc<RwLock<Inner>>`，
    // 内含 `Vec<Arc<dyn SyncTask>>`）去 Send/Sync 化，波及全栈。这里把 `!Send` 工作
    // 搬进 `spawn_local`，对外只等待 oneshot 接收端（`Result<_, FlareError>` 是 Send），
    // 从而返回一个 Send future —— transport 边界一处收口，上层维持 Send + Sync。
    let method = method.to_string();
    let url = append_query(url, query);
    let content_type = content_type.map(|s| s.to_string());
    let extra_headers = extra_headers.cloned();

    let (tx, rx) = futures::channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let result = fetch_bytes_with_headers_local(
            &method,
            url,
            body,
            content_type.as_deref(),
            context_headers,
            extra_headers.as_ref(),
        )
        .await;
        // 接收端被丢弃（调用方取消）时忽略发送失败。
        let _ = tx.send(result);
    });
    rx.await
        .map_err(|_| FlareError::system("http fetch task dropped before completion"))?
}

/// `!Send` 内部实现：真正执行浏览器 `fetch`。仅经 [`fetch_bytes_with_headers`]
/// 在 `spawn_local` 内调用，`url` 已在外层完成 query 拼接。
async fn fetch_bytes_with_headers_local(
    method: &str,
    url: String,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
    context_headers: HashMap<String, String>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<(Vec<u8>, HashMap<String, String>)> {
    let window =
        web_sys::window().ok_or_else(|| FlareError::system("browser window unavailable"))?;
    let headers = Headers::new()
        .map_err(|e| FlareError::system(format!("http headers init failed: {e:?}")))?;
    apply_headers(&headers, &context_headers, extra_headers)?;
    if let Some(content_type) = content_type {
        headers
            .set("Content-Type", content_type)
            .map_err(|e| FlareError::system(format!("http set content-type failed: {e:?}")))?;
    }

    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::Cors);
    init.set_headers(&headers);
    if let Some(body) = body {
        let array = Uint8Array::from(body.as_slice());
        init.set_body(array.as_ref());
    }

    let request = Request::new_with_str_and_init(&url, &init)
        .map_err(|e| FlareError::system(format!("http request init failed: {e:?}")))?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| FlareError::system(format!("http fetch failed: {method} {url}: {e:?}")))?;
    let resp: Response = resp_value
        .dyn_into()
        .map_err(|e| FlareError::system(format!("http response cast failed: {e:?}")))?;
    let status = resp.status();
    if !(200..300).contains(&status) {
        let body = JsFuture::from(
            resp.text()
                .map_err(|e| FlareError::system(format!("http read error body failed: {e:?}")))?,
        )
        .await
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();
        return Err(http_error_from_response_status(status, &body));
    }
    let response_headers = collect_response_headers(resp.headers())?;
    let buffer = JsFuture::from(
        resp.array_buffer()
            .map_err(|e| FlareError::system(format!("http read body failed: {e:?}")))?,
    )
    .await
    .map_err(|e| FlareError::system(format!("http read body failed: {e:?}")))?;
    let array = Uint8Array::new(&buffer);
    Ok((array.to_vec(), response_headers))
}

fn collect_response_headers(headers: Headers) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    let iterator = js_sys::try_iter(headers.as_ref())
        .map_err(|e| FlareError::system(format!("http response headers iter failed: {e:?}")))?
        .ok_or_else(|| FlareError::system("http response headers are not iterable"))?;
    for item in iterator {
        let entry = item
            .map_err(|e| FlareError::system(format!("http response header item failed: {e:?}")))?;
        let pair = js_sys::Array::from(&entry);
        if pair.length() < 2 {
            continue;
        }
        let key = pair.get(0).as_string().unwrap_or_default();
        if key.is_empty() {
            continue;
        }
        let value = pair.get(1).as_string().unwrap_or_default();
        out.insert(key, value);
    }
    Ok(out)
}

pub async fn fetch_json<T>(
    method: &str,
    url: String,
    query: Option<&HashMap<String, String>>,
    body: Option<&str>,
    context_headers: HashMap<String, String>,
    extra_headers: Option<&HashMap<String, String>>,
) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let bytes = fetch_bytes(
        method,
        url,
        query,
        body.map(|s| s.as_bytes().to_vec()),
        body.map(|_| "application/json"),
        context_headers,
        extra_headers,
    )
    .await?;
    serde_json::from_slice(&bytes)
        .map_err(|e| FlareError::system(format!("parse http json failed: {e}")))
}
