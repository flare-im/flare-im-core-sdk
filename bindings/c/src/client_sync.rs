//! 与 [`flare_im_core_sdk::client::IMClient`] 直接对齐的 C API。

use std::ffi::{c_char, c_void};

use crate::abi;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::c_str_to_string;
use crate::registry::{require_instance, retain_instance};
use crate::types::{FlareHandle, FlareResultCallback};

#[repr(i32)]
#[derive(Clone, Copy)]
pub enum FlareSdkStateCode {
    Disconnected = 0,
    Connecting = 1,
    Connected = 2,
    Ready = 3,
    Reconnecting = 4,
}

fn map_sdk_state(s: flare_im_core_sdk::core::SdkState) -> i32 {
    use flare_im_core_sdk::core::SdkState as S;
    match s {
        S::Disconnected => FlareSdkStateCode::Disconnected as i32,
        S::Connecting => FlareSdkStateCode::Connecting as i32,
        S::Connected => FlareSdkStateCode::Connected as i32,
        S::Ready => FlareSdkStateCode::Ready as i32,
        S::Reconnecting => FlareSdkStateCode::Reconnecting as i32,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_state(handle: FlareHandle) -> i32 {
    abi::catch_ffi_i32(|| {
        retain_instance(handle).map_or(FlareSdkStateCode::Disconnected as i32, |instance| {
            map_sdk_state(instance.client.state())
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_disconnect(
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
        execute_async_unit(instance, ctx, async move { client.disconnect().await });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_mark_session_read(
    handle: FlareHandle,
    conversation_id: *const c_char,
    read_seq: u64,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async_unit(instance, ctx, async move {
            client.mark_session_read(&conversation_id, read_seq).await
        });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_sync_conversation(
    handle: FlareHandle,
    conversation_id: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async_unit(instance, ctx, async move {
            client.sync_conversation(&conversation_id).await
        });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_sync_messages(
    handle: FlareHandle,
    conversation_id: *const c_char,
    last_seq: u64,
    limit: i32,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async_unit(instance, ctx, async move {
            client
                .sync_messages(&conversation_id, last_seq, limit)
                .await
        });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_set_conversation_input_state(
    handle: FlareHandle,
    conversation_id: *const c_char,
    is_typing: bool,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let conversation_id = match c_str_to_string(conversation_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid conversation_id");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async_unit(instance, ctx, async move {
            client
                .set_conversation_input_state(&conversation_id, is_typing)
                .await
        });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_get_user_presence(
    handle: FlareHandle,
    user_id: *const c_char,
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
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async(
            instance,
            ctx,
            async move { client.get_user_presence(&user_id).await },
            |value| serde_json::to_string(&value).map_err(|_| -1),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_batch_get_user_presence(
    handle: FlareHandle,
    user_ids_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let user_ids_json = match c_str_to_string(user_ids_json) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid user_ids_json");
                return code;
            }
        };
        let user_ids = match serde_json::from_str::<Vec<String>>(&user_ids_json) {
            Ok(ids) => ids,
            Err(_) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, -1, "Invalid user_ids_json");
                return -1;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async(
            instance,
            ctx,
            async move { client.batch_get_user_presence(&user_ids).await },
            |value| serde_json::to_string(&value).map_err(|_| -1),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_subscribe_user_presence(
    handle: FlareHandle,
    user_ids_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let user_ids_json = match c_str_to_string(user_ids_json) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid user_ids_json");
                return code;
            }
        };
        let user_ids = match serde_json::from_str::<Vec<String>>(&user_ids_json) {
            Ok(ids) => ids,
            Err(_) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, -1, "Invalid user_ids_json");
                return -1;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();
        execute_async_unit(instance, ctx, async move {
            client.subscribe_user_presence(user_ids).await
        });
        0
    })
}
