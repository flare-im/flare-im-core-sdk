//! C ABI: FlareIMClient 包装
//!
//! 提供 C 兼容的 API，供各平台自动生成绑定
//!
//! ## 安全性说明
//!
//! 此模块包含 FFI 代码，虽然使用了 `#[no_mangle]` 和原始指针，
//! 但所有公共 API 都是安全的。所有 unsafe 操作都封装在 `safe` 模块中。

#![allow(unsafe_code)] // FFI 需要 unsafe，但已封装在安全包装层中

use crate::api::FlareIMClient;
use crate::ffi::safe;
use crate::shared::config::ClientConfig;
use prost::Message as ProstMessage;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

/// 客户端存储（用于管理客户端生命周期）
lazy_static::lazy_static! {
    static ref CLIENTS: TokioMutex<HashMap<u64, Arc<FlareIMClient>>> = TokioMutex::new(HashMap::new());
    static ref CLIENT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    static ref RUNTIME: Arc<tokio::runtime::Runtime> = Arc::new(
        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime")
    );
}

// 获取或创建运行时（线程安全）
fn get_runtime() -> Arc<tokio::runtime::Runtime> {
    RUNTIME.clone()
}

/// C ABI: 创建客户端
///
/// # 参数
/// - `config_json`: JSON 格式的配置字符串
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
///
/// # 返回
/// - 客户端句柄（如果成功，通过 callback 返回）
#[no_mangle]
pub extern "C" fn flare_im_client_new(
    config_json: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    // 使用安全的包装函数，避免显式使用 unsafe
    let config = match safe::safe_config_from_json(config_json) {
        Ok(c) => c,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    // 异步创建客户端
    // 使用 Send 安全的回调包装，避免 Send trait 问题
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        match FlareIMClient::new(config).await {
            Ok(client) => {
                let handle = CLIENT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut clients = CLIENTS.lock().await;
                clients.insert(handle, Arc::new(client));

                let result_json = format!("{{\"handle\":{}}}", handle);
                send_callback.call(Some(&result_json), None);
            }
            Err(e) => {
                send_callback.call(None, Some(e.into()));
            }
        }
    });
}

/// C ABI: 登录
///
/// # 参数
/// - `handle`: 客户端句柄
/// - `user_id`: 用户 ID
/// - `token`: 认证 Token
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_client_login(
    handle: u64,
    user_id: *const c_char,
    token: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    // 使用安全的包装函数
    let user_id_str = match safe::c_str_to_string(user_id) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    let token_str = match safe::c_str_to_string(token) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    // 使用 Send 安全的回调包装
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let clients = CLIENTS.lock().await;
        if let Some(client) = clients.get(&handle) {
            match client.login(&user_id_str, &token_str).await {
                Ok(result) => {
                    // LoginResult 需要手动序列化
                    let result_json = format!(
                        "{{\"user_id\":\"{}\",\"session_id\":\"{}\"}}",
                        result.user_id, result.session_id
                    );
                    send_callback.call(Some(&result_json), None);
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("Client not found").into()));
        }
    });
}

/// C ABI: 发送文本消息
///
/// # 参数
/// - `handle`: 客户端句柄
/// - `session_id`: 会话 ID
/// - `text`: 消息文本
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_client_send_message(
    handle: u64,
    session_id: *const c_char,
    text: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    if session_id.is_null() || text.is_null() {
        let error = CString::new("session_id or text is null").unwrap();
        callback(user_data, ptr::null(), error.as_ptr());
        return;
    }

    // 使用安全的包装函数
    let session_id_str = match safe::c_str_to_string(session_id) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    let text_str = match safe::c_str_to_string(text) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    // 使用 Send 安全的回调包装
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let clients = CLIENTS.lock().await;
        if let Some(client) = clients.get(&handle) {
            let content = flare_proto::MessageContent {
                content: Some(
                    flare_proto::flare::common::v1::message_content::Content::Text(
                        flare_proto::TextContent {
                            text: text_str,
                            mentions: vec![],
                        },
                    ),
                ),
            };

            match client.send_message(&session_id_str, content).await {
                Ok(message_id) => {
                    let result_json = format!("{{\"message_id\":\"{}\"}}", message_id);
                    send_callback.call(Some(&result_json), None);
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("Client not found").into()));
        }
    });
}

/// C ABI: 获取会话列表
///
/// # 参数
/// - `handle`: 客户端句柄
/// - `filter_json`: JSON 格式的过滤条件（可选）
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_client_get_sessions(
    handle: u64,
    _filter_json: *const c_char, // 暂时未使用，保留用于将来扩展
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    // 使用 Send 安全的回调包装
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let clients = CLIENTS.lock().await;
        if let Some(client) = clients.get(&handle) {
            // 简化：直接使用默认过滤器（JSON 解析需要 SessionFilter 实现 Deserialize，暂时跳过）
            let filter = crate::infrastructure::repository::SessionFilter::new();

            match client.get_sessions(filter).await {
                Ok(sessions) => {
                    // 手动序列化会话列表
                    let mut sessions_json = String::from("[");
                    for (i, session) in sessions.iter().enumerate() {
                        if i > 0 {
                            sessions_json.push(',');
                        }
                        sessions_json.push_str(&format!(
                            "{{\"session_id\":\"{}\",\"session_type\":\"{}\",\"business_type\":\"{}\",\"unread_count\":{}}}",
                            session.session_id,
                            session.session_type,
                            session.business_type,
                            session.unread_count
                        ));
                    }
                    sessions_json.push(']');
                    send_callback.call(Some(&sessions_json), None);
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("Client not found").into()));
        }
    });
}

/// C ABI: 获取消息列表
///
/// # 参数
/// - `handle`: 客户端句柄
/// - `session_id`: 会话 ID
/// - `limit`: 限制数量
/// - `cursor`: 游标（可选，可为 null）
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_client_get_messages(
    handle: u64,
    session_id: *const c_char,
    limit: u32,
    cursor: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    if session_id.is_null() {
        let error = CString::new("session_id is null").unwrap();
        callback(user_data, ptr::null(), error.as_ptr());
        return;
    }

    // 使用安全的包装函数
    let session_id_str = match safe::c_str_to_string(session_id) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    let cursor_opt = if cursor.is_null() {
        None
    } else {
        safe::c_str_to_string(cursor).ok()
    };

    // 使用 Send 安全的回调包装
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let clients = CLIENTS.lock().await;
        if let Some(client) = clients.get(&handle) {
            match client
                .get_messages(&session_id_str, limit as usize, cursor_opt)
                .await
            {
                Ok(messages) => {
                    // 使用 protobuf 序列化消息列表
                    // 将 Vec<Message> 转换为 protobuf 格式的字节数组，然后 base64 编码
                    let mut result_bytes = Vec::new();
                    for message in &messages {
                        // 使用 encode_to_vec 方法序列化每个消息
                        // Message 是 flare_proto::Message，它实现了 prost::Message
                        let message_bytes = ProstMessage::encode_to_vec(message);
                        // 先写入消息长度（4字节，大端序），然后写入消息内容
                        result_bytes.extend_from_slice(&(message_bytes.len() as u32).to_be_bytes());
                        result_bytes.extend_from_slice(&message_bytes);
                    }

                    // 使用 base64 编码字节数组，方便在 JSON 中传输
                    // 使用 base64 crate 的新 API
                    use base64::{Engine as _, engine::general_purpose};
                    let result_json = if result_bytes.is_empty() {
                        "[]".to_string()
                    } else {
                        let encoded = general_purpose::STANDARD.encode(&result_bytes);
                        format!("{{\"messages_base64\":\"{}\"}}", encoded)
                    };

                    send_callback.call(Some(&result_json), None);
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("Client not found").into()));
        }
    });
}

/// C ABI: 释放客户端
///
/// # 参数
/// - `handle`: 客户端句柄
#[no_mangle]
pub extern "C" fn flare_im_client_free(handle: u64) {
    let rt = get_runtime();
    rt.spawn(async move {
        let mut clients = CLIENTS.lock().await;
        clients.remove(&handle);
    });
}
