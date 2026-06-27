//! 生命周期 API - SDK 初始化、登录、登出
//!
//! 本模块仅做句柄/字符串/回调编排，业务在 `flare_im_core_sdk`。

use std::ffi::{c_char, c_void};
use std::sync::Arc;

use flare_im_core_sdk::prelude::SdkConfigOverlay;
#[cfg(feature = "dev-test-token")]
use flare_im_core_sdk::prelude::{
    CoreTokenConfig, generate_core_token as util_generate_core_token,
};
use flare_im_core_sdk::prelude::{IMClient, LoginDbKind};

use crate::abi;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{
    c_str_to_string, parse_init_request, parse_json, string_to_flare, to_json_string,
};
use crate::registry::{
    SdkInstance, register_instance, release_all_instances, release_instance, require_instance,
    retain_instance,
};
use crate::types::{FlareHandle, FlareResultCallback, FlareString};

pub const FLARE_FFI_CONTRACT_VERSION: &str =
    flare_im_core_sdk_bindings_runtime::BINDING_CONTRACT_VERSION;

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_create() -> FlareHandle {
    abi::catch_ffi_handle(|| {
        let client = IMClient::new();
        let runtime = match crate::ffi_runtime::sdk_runtime_handle() {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::error!(target: "flare_im_ffi", %error, "failed to create SDK instance");
                return 0;
            }
        };
        let instance = Arc::new(SdkInstance {
            client,
            runtime,
            im_session: crate::session::ImSessionSlot::default(),
        });
        register_instance(instance)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_release(handle: FlareHandle) {
    abi::catch_ffi_void(|| {
        crate::event::unsubscribe_events_for_handle(handle);
        release_instance(handle);
    });
}

/// FFI 全局硬重置：取消全部事件订阅并释放全部 SDK 句柄。
///
/// 用途：Flutter/iOS 热重启后，旧 isolate 的回调地址可能失效。
/// 在新 isolate 初始化前调用可避免旧后台任务继续回调导致崩溃。
#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_hard_reset() {
    abi::catch_ffi_void(|| {
        crate::event::unsubscribe_all_events();
        release_all_instances();
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_init(
    handle: FlareHandle,
    config_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let (environment, config) = match parse_init_request(config_json) {
            Ok(c) => c,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Failed to parse config JSON");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();

        execute_async_unit(instance, ctx, async move {
            client.init(environment, Some(config)).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_uninit(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        let session = instance.im_session.clone();

        execute_async_unit(instance, ctx, async move {
            session.clear().await;
            client.uninit().await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_login(
    handle: FlareHandle,
    user_id: *const c_char,
    token: *const c_char,
    store_config_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let user_id = match c_str_to_string(user_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid user_id");
                return code;
            }
        };

        let token = match c_str_to_string(token) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid token");
                return code;
            }
        };

        let login_config = if store_config_json.is_null() {
            None
        } else {
            match parse_json::<SdkConfigOverlay>(store_config_json) {
                Ok(config) => Some(config),
                Err(code) => {
                    let ctx = CallbackContext::new(context, callback);
                    return_error(&ctx, code, "Invalid store_config JSON");
                    return code;
                }
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            inst.im_session.clear().await;
            if let Some(config) = login_config {
                inst.client.init(None, Some(config)).await?;
                inst.im_session.clear().await;
            }
            let apis = inst
                .client
                .login(
                    &user_id,
                    Some(token.as_str()),
                    LoginDbKind::Sqlite,
                    |_, _| {},
                )
                .await?;
            inst.im_session.install(&inst.client, apis).await;
            Ok(())
        });

        0
    })
}

/// 预热登录会话：开 per-user 本地库 + 装配引擎，**不连网**。
///
/// 与 [`flare_sdk_connect`] 配合实现「初始化前置、登录只做网络」：App 启动即可对
/// 「上次登录用户」调用本函数，把开库 / 迁移 / 建引擎移出登录关键路径。
#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_prepare(
    handle: FlareHandle,
    user_id: *const c_char,
    store_config_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let user_id = match c_str_to_string(user_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid user_id");
                return code;
            }
        };

        let prepare_config = if store_config_json.is_null() {
            None
        } else {
            match parse_json::<SdkConfigOverlay>(store_config_json) {
                Ok(config) => Some(config),
                Err(code) => {
                    let ctx = CallbackContext::new(context, callback);
                    return_error(&ctx, code, "Invalid store_config JSON");
                    return code;
                }
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            inst.im_session.clear().await;
            if let Some(config) = prepare_config {
                inst.client.init(None, Some(config)).await?;
                inst.im_session.clear().await;
            }
            inst.client.prepare(&user_id, LoginDbKind::Sqlite).await
        });

        0
    })
}

/// 连接已 [`flare_sdk_prepare`] 预热好的会话并完成首次同步（登录的网络半段）。
#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_connect(
    handle: FlareHandle,
    user_id: *const c_char,
    token: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let user_id = match c_str_to_string(user_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid user_id");
                return code;
            }
        };

        let token = match c_str_to_string(token) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid token");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            let apis = inst.client.connect(&user_id, Some(token.as_str())).await?;
            inst.im_session.install(&inst.client, apis).await;
            Ok(())
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_logout(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async_unit(instance, ctx, async move {
            inst.im_session.clear().await;
            inst.client.logout().await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_update_access_token(
    handle: FlareHandle,
    access_token: *const c_char,
    tenant_id: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let access_token = match c_str_to_string(access_token) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid access_token");
                return code;
            }
        };

        let tenant_id = match abi::read_c_str_opt(tenant_id) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid tenant_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();

        execute_async_unit(instance, ctx, async move {
            client
                .update_access_token(access_token, tenant_id.as_deref())
                .await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_version() -> FlareString {
    abi::catch_ffi_flare_string(|| string_to_flare(env!("CARGO_PKG_VERSION").to_string()))
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_ffi_contract_version() -> FlareString {
    abi::catch_ffi_flare_string(|| string_to_flare(FLARE_FFI_CONTRACT_VERSION.to_string()))
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_data_root(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();

        execute_async(
            instance,
            ctx,
            async move {
                Ok(client
                    .data_root()
                    .await
                    .map(|path| path.display().to_string())
                    .unwrap_or_default())
            },
            |data_root| to_json_string(&serde_json::json!({ "dataRoot": data_root })),
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_is_connected(handle: FlareHandle) -> bool {
    abi::catch_ffi_bool(|| {
        retain_instance(handle).is_some_and(|instance| {
            matches!(instance.client.state(), flare_im_core_sdk::SdkState::Ready)
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_session_active(handle: FlareHandle) -> bool {
    abi::catch_ffi_bool(|| {
        retain_instance(handle).is_some_and(|instance| instance.client.session_active_sync())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_current_user_id(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let user_id = client.current_user_id().await;
                Ok(user_id.unwrap_or_default())
            },
            Ok,
        );

        0
    })
}

#[cfg(feature = "dev-test-token")]
#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_generate_core_token(
    secret: *const c_char,
    issuer: *const c_char,
    user_id: *const c_char,
    device_id: *const c_char,
    tenant_id: *const c_char,
    ttl_secs: u64,
) -> FlareString {
    abi::catch_ffi_flare_string(|| {
        let user_id = match c_str_to_string(user_id) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                tracing::warn!("flare_sdk_generate_core_token: empty user_id");
                return FlareString::default();
            }
            Err(_) => return FlareString::default(),
        };

        let secret_s = match abi::read_c_str_opt(secret) {
            Ok(o) => o.unwrap_or_default(),
            Err(_) => return FlareString::default(),
        };
        let issuer_s = match abi::read_c_str_opt(issuer) {
            Ok(o) => o.unwrap_or_default(),
            Err(_) => return FlareString::default(),
        };

        let device = match abi::read_c_str_opt(device_id) {
            Ok(o) => o.filter(|s| !s.is_empty()),
            Err(_) => return FlareString::default(),
        };

        let tenant = match abi::read_c_str_opt(tenant_id) {
            Ok(o) => o.filter(|s| !s.is_empty()),
            Err(_) => return FlareString::default(),
        };

        match util_generate_core_token(&CoreTokenConfig {
            secret: secret_s,
            issuer: issuer_s,
            user_id,
            ttl_secs,
            device_id: device,
            tenant_id: tenant,
        }) {
            Ok(token) => string_to_flare(token),
            Err(e) => {
                tracing::warn!(error = %e, "flare_sdk_generate_core_token failed");
                FlareString::default()
            }
        }
    })
}
