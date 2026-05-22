//! 媒体 API - 透传 `MediaApi`。

use std::ffi::{c_char, c_void};

use crate::abi;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{c_str_to_string, to_json_string};
use crate::registry::require_instance;
use crate::types::{FlareHandle, FlareResultCallback};

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_get_url(
    handle: FlareHandle,
    media_id: *const c_char,
    expires_in: i32,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };

        let media_id = match c_str_to_string(media_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid media_id");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();

        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.media_api().await?;
                api.get_file_url(&media_id, expires_in).await
            },
            |url| to_json_string(&url),
        );

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_set_cache_max_bytes(
    handle: FlareHandle,
    max_bytes: u64,
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
            let api = inst.media_api().await?;
            api.set_media_cache_max_bytes(max_bytes).await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_clear_cache(
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
            let api = inst.media_api().await?;
            api.clear_media_cache().await
        });

        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_resolve_access(
    handle: FlareHandle,
    file_id: *const c_char,
    expires_in: i32,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let file_id = match c_str_to_string(file_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid file_id");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.media_api().await?;
                api.resolve_media_access(&file_id, expires_in).await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_cache_remote(
    handle: FlareHandle,
    file_id: *const c_char,
    expires_in: i32,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let file_id = match c_str_to_string(file_id) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid file_id");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.media_api().await?;
                api.cache_remote_media(&file_id, expires_in).await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_cache_stats(
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
        execute_async(
            instance,
            ctx,
            async move {
                let api = inst.media_api().await?;
                api.media_cache_stats().await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_set_cache_root(
    handle: FlareHandle,
    absolute_path: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let path = match abi::read_c_str_opt(absolute_path) {
            Ok(o) => o,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid absolute_path");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async_unit(instance, ctx, async move {
            let api = inst.media_api().await?;
            api.set_media_cache_root(path.as_deref()).await
        });
        0
    })
}
