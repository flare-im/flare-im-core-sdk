//! 生命周期 API 模块
//!
//! 实现 SDK 的创建、销毁、初始化、登录、登出、状态查询等功能

use std::ffi::c_void;
use std::sync::Arc;

use flare_im_core_sdk::client::IMClient;
use flare_im_core_sdk::lifecycle::SdkConfigOverlay;
use flare_im_core_sdk::prelude::SdkState;

use crate::callback::{invoke_result_callback, CallbackContext, FlareResultCallback};
use crate::error::FlareErrorCode;
use crate::handle::{get_instance, next_handle_id, register_instance, remove_instance, FlareImHandle, SdkInstance};
use crate::json::parse_json;
use crate::string::{parse_string, string_to_c};

/// 创建 SDK 实例
///
/// # Returns
/// SDK 句柄，id > 0 表示成功，id == 0 表示失败
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_new() -> FlareImHandle {
    match flare_im_new_inner() {
        Ok(handle) => handle,
        Err(e) => {
            tracing::error!("Failed to create SDK instance: {:?}", e);
            FlareImHandle::default()
        }
    }
}

fn flare_im_new_inner() -> Result<FlareImHandle, FlareErrorCode> {
    // 生成句柄 ID
    let id = next_handle_id();

    // 创建 IMClient
    let client = IMClient::new();

    // 获取当前 Tokio runtime handle
    let runtime = tokio::runtime::Handle::try_current().map_err(|e| {
        tracing::error!("No Tokio runtime available: {}", e);
        FlareErrorCode::InternalError
    })?;

    // 创建实例
    let instance = Arc::new(SdkInstance {
        client,
        runtime,
        event_subscriptions: std::sync::RwLock::new(Vec::new()),
    });

    // 注册到句柄表
    register_instance(id, instance)?;

    Ok(FlareImHandle { id })
}

/// 释放 SDK 实例
///
/// # Arguments
/// * `handle` - SDK 句柄
///
/// # Note
/// 释放后会断开连接并清理所有资源
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_free(handle: FlareImHandle) {
    match flare_im_free_inner(handle) {
        Ok(()) => {}
        Err(e) => tracing::error!("Failed to free SDK instance: {:?}", e),
    }
}

fn flare_im_free_inner(handle: FlareImHandle) -> Result<(), FlareErrorCode> {
    // 从句柄表移除实例
    let instance = remove_instance(handle)?;

    // 取消所有事件订阅
    if let Ok(mut subs) = instance.event_subscriptions.write() {
        for sub in subs.drain(..) {
            let _ = sub.cancel_tx.send(());
        }
    }

    Ok(())
}

/// 初始化 SDK
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `config_json` - 配置 JSON，格式见 SdkConfig
/// * `context` - 用户上下文，将传递给 callback
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_init(
    handle: FlareImHandle,
    config_json: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = flare_im_init_inner(handle, config_json, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to init SDK: {:?}", e);
        // 直接调用回调返回错误
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn flare_im_init_inner(
    handle: FlareImHandle,
    config_json: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    // 获取实例
    let instance = get_instance(handle)?;

    // 解析配置
    let config: SdkConfigOverlay = parse_json(config_json)?;

    // 创建回调上下文
    let ctx = CallbackContext {
        user_context: context,
        callback,
    };

    // 克隆客户端
    let client = instance.client.clone();

    // 在 Tokio runtime 中执行异步初始化
    instance.runtime.spawn(async move {
        let result = client.init(None, Some(config)).await.map_err(|e| {
            tracing::error!("Init failed: {}", e);
            FlareErrorCode::from(&e)
        });
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 登录
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `user_id` - 用户 ID
/// * `token` - JWT Token
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_login(
    handle: FlareImHandle,
    user_id: *const i8,
    token: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    let result = flare_im_login_inner(handle, user_id, token, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to login: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn flare_im_login_inner(
    handle: FlareImHandle,
    user_id: *const i8,
    token: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    // 获取实例
    let instance = get_instance(handle)?;

    // 解析参数
    let user_id = parse_string(user_id)?;
    let token = parse_string(token)?;

    // 创建回调上下文
    let ctx = CallbackContext {
        user_context: context,
        callback,
    };

    // 克隆客户端
    let client = instance.client.clone();

    // 在 Tokio runtime 中执行异步登录
    instance.runtime.spawn(async move {
        // 使用 IndexedDb 作为存储类型 (需要 StoreProvider)
        // 注意: 这里需要根据实际情况提供 StoreProvider
        // 暂时使用一个空的实现
        let result = Err(FlareErrorCode::InternalError);

        // TODO: 实现正确的登录逻辑
        // let result = client
        //     .login(&user_id, Some(&token), LoginDbKind::IndexedDb(store_provider), |_bus, _| {
        //         // 事件转发在 subscribe_events 中处理
        //     })
        //     .await
        //     .map_err(|e| {
        //         tracing::error!("Login failed: {}", e);
        //         FlareErrorCode::from(&e)
        //     });
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 登出
///
/// # Arguments
/// * `handle` - SDK 句柄
/// * `context` - 用户上下文
/// * `callback` - 结果回调
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_logout(handle: FlareImHandle, context: *mut c_void, callback: FlareResultCallback) {
    let result = flare_im_logout_inner(handle, context, callback);
    if let Err(e) = result {
        tracing::error!("Failed to logout: {:?}", e);
        let ctx = CallbackContext {
            user_context: context,
            callback,
        };
        invoke_result_callback(ctx, Err(e));
    }
}

fn flare_im_logout_inner(
    handle: FlareImHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> Result<(), FlareErrorCode> {
    // 获取实例
    let instance = get_instance(handle)?;

    // 创建回调上下文
    let ctx = CallbackContext {
        user_context: context,
        callback,
    };

    // 克隆客户端
    let client = instance.client.clone();

    // 在 Tokio runtime 中执行异步登出
    instance.runtime.spawn(async move {
        let result = client.logout().await.map_err(|e| {
            tracing::error!("Logout failed: {}", e);
            FlareErrorCode::from(&e)
        });
        invoke_result_callback(ctx, result);
    });

    Ok(())
}

/// 是否已连接（同步）
///
/// # Arguments
/// * `handle` - SDK 句柄
///
/// # Returns
/// true 表示已连接，false 表示未连接或无效句柄
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_is_connected(handle: FlareImHandle) -> bool {
    get_instance(handle)
        .ok()
        .map(|i| i.client.state() == SdkState::Ready)
        .unwrap_or(false)
}

/// 当前用户 ID（同步）
///
/// # Arguments
/// * `handle` - SDK 句柄
///
/// # Returns
/// 用户 ID 字符串，需调用 flare_im_string_free 释放；未登录返回 NULL
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_current_user_id(handle: FlareImHandle) -> *const i8 {
    match flare_im_current_user_id_inner(handle) {
        Ok(Some(user_id)) => string_to_c(user_id),
        Ok(None) => std::ptr::null(),
        Err(e) => {
            tracing::error!("Failed to get current user id: {:?}", e);
            std::ptr::null()
        }
    }
}

fn flare_im_current_user_id_inner(handle: FlareImHandle) -> Result<Option<String>, FlareErrorCode> {
    let instance = get_instance(handle)?;
    Ok(instance.client.current_user_id())
}

/// SDK 状态（同步）
///
/// # Arguments
/// * `handle` - SDK 句柄
///
/// # Returns
/// 状态字符串，如 "Ready"、"Disconnected"、"Reconnecting"；无效句柄返回 NULL
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_state(handle: FlareImHandle) -> *const i8 {
    match flare_im_state_inner(handle) {
        Ok(state) => string_to_c(state),
        Err(e) => {
            tracing::error!("Failed to get state: {:?}", e);
            std::ptr::null()
        }
    }
}

fn flare_im_state_inner(handle: FlareImHandle) -> Result<String, FlareErrorCode> {
    let instance = get_instance(handle)?;
    Ok(format!("{:?}", instance.client.state()))
}
