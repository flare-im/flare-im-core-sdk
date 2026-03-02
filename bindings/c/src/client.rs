//! C ABI: SDK 包装层
//!
//! 提供 C 兼容的 API，供各平台自动生成绑定
//! 
//! ## 架构设计
//!
//! FFI 层作为适配层，负责：
//! - 类型转换：C 类型 ↔ JSON ↔ Interface API（领域模型）
//! - Callback 管理：将 async 结果通过 callback 返回
//! - 生命周期管理：管理 SDK 实例的创建和销毁
//!
//! ## 安全性说明
//!
//! 此模块包含 FFI 代码，虽然使用了 `#[no_mangle]` 和原始指针，
//! 但所有公共 API 都是安全的。所有 unsafe 操作都封装在 `safe` 模块中。

#![allow(unsafe_code)] // FFI 需要 unsafe，但已封装在安全包装层中

use flare_im_core_sdk::interface::facade::ImCoreSdk;
use flare_im_core_sdk::config::SdkConfig;
use flare_im_core_sdk::infrastructure::converter::ConverterRegistry;
use flare_im_core_sdk::application::queries::ListMessagesQuery;
use crate::safe;
use serde_json::Value;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

/// SDK 实例存储（用于管理生命周期）
lazy_static::lazy_static! {
    static ref SDK_INSTANCES: TokioMutex<HashMap<u64, Arc<ImCoreSdk>>> = TokioMutex::new(HashMap::new());
    static ref SDK_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    static ref RUNTIME: Arc<tokio::runtime::Runtime> = Arc::new(
        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime")
    );
}

// 获取或创建运行时（线程安全）
fn get_runtime() -> Arc<tokio::runtime::Runtime> {
    RUNTIME.clone()
}

// 辅助函数：从 JSON 创建 SdkConfig
fn parse_sdk_config_from_json(json_str: &str) -> Result<SdkConfig, String> {
    // 尝试直接解析为 SdkConfig
    if let Ok(config) = serde_json::from_str::<SdkConfig>(json_str) {
        return Ok(config);
    }
    
    // 如果失败，尝试使用 builder 模式（兼容旧格式）
    let value: Value = serde_json::from_str(json_str)
        .map_err(|e| format!("Invalid JSON: {}", e))?;
    
    let mut builder = SdkConfig::builder();
    
    if let Some(ws_url) = value.get("websocket_url").and_then(|v| v.as_str()) {
        builder = builder.websocket_url(ws_url);
    }
    
    if let Some(storage_path) = value.get("storage_path").and_then(|v| v.as_str()) {
        builder = builder.storage_path(storage_path);
    }
    
    if let Some(media_cache_path) = value.get("media_cache_path").and_then(|v| v.as_str()) {
        builder = builder.media_cache_path(media_cache_path);
    }
    
    if let Some(log_level) = value.get("log_level").and_then(|v| v.as_str()) {
        builder = builder.log_level(log_level);
    }
    
    Ok(builder.build())
}

/// C ABI: 创建 SDK 实例
///
/// # 参数
/// - `config_json`: JSON 格式的配置字符串
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
///
/// # 返回
/// - SDK 句柄（如果成功，通过 callback 返回）
#[no_mangle]
pub extern "C" fn flare_im_sdk_new(
    config_json: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    // 解析配置 JSON
    let config_str = match safe::c_str_to_string(config_json) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };
    
    // 从 JSON 创建 SdkConfig
    let config = match parse_sdk_config_from_json(&config_str) {
        Ok(c) => c,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    // 异步创建 SDK 实例
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        match ImCoreSdk::new(config).await {
            Ok(sdk) => {
                let handle = SDK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut instances = SDK_INSTANCES.lock().await;
                instances.insert(handle, Arc::new(sdk));

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
/// - `handle`: SDK 句柄
/// - `user_id`: 用户 ID
/// - `token`: 认证 Token
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_sdk_login(
    handle: u64,
    user_id: *const c_char,
    token: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
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

    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let instances = SDK_INSTANCES.lock().await;
        if let Some(sdk) = instances.get(&handle) {
            match sdk.login(user_id_str, token_str).await {
                Ok(_) => {
                    let result_json = r#"{"success":true}"#;
                    send_callback.call(Some(result_json), None);
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("SDK instance not found").into()));
        }
    });
}

/// C ABI: 创建文本消息
///
/// # 参数
/// - `handle`: SDK 句柄
/// - `conversation_id`: 会话 ID
/// - `sender_id`: 发送者 ID
/// - `text`: 消息文本
/// - `tenant_json`: 租户上下文 JSON
/// - `receiver_id`: 接收者 ID（可选，单聊必需）
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_sdk_create_text_message(
    handle: u64,
    conversation_id: *const c_char,
    sender_id: *const c_char,
    text: *const c_char,
    tenant_json: *const c_char,
    receiver_id: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    // 解析参数
    let conv_id = match safe::c_str_to_string(conversation_id) {
        Ok(s) => s,
        Err(e) => {
            safe::call_callback(callback, user_data, None, Some(&e));
            return;
        }
    };
    
    let sender = match safe::c_str_to_string(sender_id) {
        Ok(s) => s,
        Err(e) => {
            safe::call_callback(callback, user_data, None, Some(&e));
            return;
        }
    };
    
    let text_str = match safe::c_str_to_string(text) {
        Ok(s) => s,
        Err(e) => {
            safe::call_callback(callback, user_data, None, Some(&e));
            return;
        }
    };
    
    // 解析租户上下文
    let tenant_str = match safe::c_str_to_string(tenant_json) {
        Ok(s) => s,
        Err(e) => {
            safe::call_callback(callback, user_data, None, Some(&e));
            return;
        }
    };
    
    let tenant: flare_im_core_sdk::domain::message::TenantContext = match serde_json::from_str(&tenant_str) {
        Ok(t) => t,
        Err(e) => {
            safe::call_callback(callback, user_data, None, Some(&format!("Invalid tenant JSON: {}", e)));
            return;
        }
    };
    
    let receiver = if receiver_id.is_null() {
        None
    } else {
        safe::c_str_to_string(receiver_id).ok()
    };
    
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let instances = SDK_INSTANCES.lock().await;
        if let Some(sdk) = instances.get(&handle) {
            // ✅ 调用 Interface 层，获取领域模型
            match sdk.message().create_text_message(conv_id, sender, text_str, tenant, receiver) {
                Ok(message) => {
                    // ✅ FFI 层统一转换为 JSON 字符串
                    let converter = ConverterRegistry::new();
                    match converter.message_to_json(message) {
                        Ok(message_json) => {
                            let result_str = serde_json::to_string(&message_json)
                                .unwrap_or_else(|_| "{}".to_string());
                            send_callback.call(Some(&result_str), None);
                        }
                        Err(e) => {
                            send_callback.call(None, Some(e.into()));
                        }
                    }
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("SDK instance not found").into()));
        }
    });
}

/// C ABI: 发送消息
///
/// # 参数
/// - `handle`: SDK 句柄
/// - `message_json`: 消息 JSON 字符串
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_sdk_send_message(
    handle: u64,
    message_json: *const c_char,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    let message_json_str = match safe::c_str_to_string(message_json) {
        Ok(s) => s,
        Err(e) => {
            safe::call_callback(callback, user_data, None, Some(&e));
            return;
        }
    };
    
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let instances = SDK_INSTANCES.lock().await;
        if let Some(sdk) = instances.get(&handle) {
            // ✅ FFI 层：JSON 字符串 → 领域模型
            let converter = ConverterRegistry::new();
            let message_value: Value = match serde_json::from_str(&message_json_str) {
                Ok(v) => v,
                Err(e) => {
                    send_callback.call(None, Some(anyhow::anyhow!("Invalid JSON: {}", e).into()));
                    return;
                }
            };
            
            let message = match converter.json_to_message(message_value) {
                Ok(m) => m,
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                    return;
                }
            };
            
            // ✅ 调用 Interface 层（接受领域模型）
            match sdk.message().send_message(message).await {
                Ok(_) => {
                    let result_json = r#"{"success":true}"#;
                    send_callback.call(Some(result_json), None);
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("SDK instance not found").into()));
        }
    });
}

/// C ABI: 获取会话列表
///
/// # 参数
/// - `handle`: SDK 句柄
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_sdk_get_conversations(
    handle: u64,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let instances = SDK_INSTANCES.lock().await;
        if let Some(sdk) = instances.get(&handle) {
            // ✅ 调用 Interface 层，获取领域模型列表
            match sdk.conversation().get_all_conversation_list().await {
                Ok(conversations) => {
                    // ✅ FFI 层统一转换为 JSON 字符串
                    let converter = ConverterRegistry::new();
                    let conversations_json: Result<Vec<Value>, _> = conversations
                        .into_iter()
                        .map(|conv| converter.conversation_to_json(conv))
                        .collect();
                    
                    match conversations_json {
                        Ok(json_vec) => {
                            let result_str = serde_json::to_string(&json_vec)
                                .unwrap_or_else(|_| "[]".to_string());
                            send_callback.call(Some(&result_str), None);
                        }
                        Err(e) => {
                            send_callback.call(None, Some(e.into()));
                        }
                    }
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("SDK instance not found").into()));
        }
    });
}

/// C ABI: 获取消息列表
///
/// # 参数
/// - `handle`: SDK 句柄
/// - `conversation_id`: 会话 ID
/// - `limit`: 限制数量
/// - `callback`: 异步回调函数 (user_data, result_json, error_json)
/// - `user_data`: 用户数据指针
#[no_mangle]
pub extern "C" fn flare_im_sdk_get_messages(
    handle: u64,
    conversation_id: *const c_char,
    limit: u32,
    callback: extern "C" fn(*mut c_void, *const c_char, *const c_char),
    user_data: *mut c_void,
) {
    if conversation_id.is_null() {
        let error = CString::new("conversation_id is null").unwrap();
        callback(user_data, ptr::null(), error.as_ptr());
        return;
    }

    let conv_id = match safe::c_str_to_string(conversation_id) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(error) = safe::string_to_c_string(&e) {
                callback(user_data, ptr::null(), error.as_ptr());
            }
            return;
        }
    };

    let send_callback = safe::SendCallback::new(callback, user_data);
    let rt = get_runtime();
    rt.spawn(async move {
        let instances = SDK_INSTANCES.lock().await;
        if let Some(sdk) = instances.get(&handle) {
            // ✅ 调用 Interface 层，获取领域模型列表
            match sdk.sdk_context().query_handler.list_messages(ListMessagesQuery {
                conversation_id: conv_id,
                limit: Some(limit as usize),
                cursor: None,
            }).await {
                Ok(messages) => {
                    // ✅ FFI 层统一转换为 JSON 字符串
                    let converter = ConverterRegistry::new();
                    let messages_json: Result<Vec<Value>, _> = messages
                        .into_iter()
                        .map(|msg| converter.message_to_json(msg))
                        .collect();
                    
                    match messages_json {
                        Ok(json_vec) => {
                            let result_str = serde_json::to_string(&json_vec)
                                .unwrap_or_else(|_| "[]".to_string());
                            send_callback.call(Some(&result_str), None);
                        }
                        Err(e) => {
                            send_callback.call(None, Some(e.into()));
                        }
                    }
                }
                Err(e) => {
                    send_callback.call(None, Some(e.into()));
                }
            }
        } else {
            send_callback.call(None, Some(anyhow::anyhow!("SDK instance not found").into()));
        }
    });
}

/// C ABI: 释放 SDK 实例
///
/// # 参数
/// - `handle`: SDK 句柄
#[no_mangle]
pub extern "C" fn flare_im_sdk_free(handle: u64) {
    let rt = get_runtime();
    rt.spawn(async move {
        let mut instances = SDK_INSTANCES.lock().await;
        instances.remove(&handle);
    });
}