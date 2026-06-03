//! 媒体 API - 透传 `MediaApi`。

use std::ffi::{c_char, c_void};

use flare_im_core_sdk::application::UserFileDownloadRequest;
use flare_im_core_sdk::model::UploadOptions;
use serde::Deserialize;

use crate::abi;
use crate::error_convert::FLARE_ERR_INVALID_PARAM;
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{c_str_to_string, to_json_string};
use crate::registry::require_instance;
use crate::types::{FlareBytesView, FlareHandle, FlareResultCallback};

fn parse_upload_options(options_json: *const c_char) -> Result<Option<UploadOptions>, i32> {
    let Some(raw) = abi::read_c_str_opt(options_json)? else {
        return Ok(None);
    };
    if raw.trim().is_empty() || raw.trim() == "null" {
        return Ok(None);
    }
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| FLARE_ERR_INVALID_PARAM)?;
    let Some(chunk_size) = value.get("chunk_size").and_then(|v| v.as_u64()) else {
        return Ok(None);
    };
    let chunk_size = usize::try_from(chunk_size).map_err(|_| FLARE_ERR_INVALID_PARAM)?;
    if chunk_size == 0 {
        return Err(FLARE_ERR_INVALID_PARAM);
    }
    Ok(Some(UploadOptions { chunk_size }))
}

#[derive(Debug, Deserialize)]
struct DownloadFileToDownloadsRequest {
    download_key: String,
    display_file_name: String,
    source_path: Option<String>,
    source_url: Option<String>,
    remote_file_id: Option<String>,
    expires_in: Option<i32>,
}

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

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_upload_file(
    handle: FlareHandle,
    path: *const c_char,
    options_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let path = match c_str_to_string(path) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid path");
                return code;
            }
        };
        let options = match parse_upload_options(options_json) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid upload options");
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
                api.upload_file_from_path(path, options).await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_upload_image(
    handle: FlareHandle,
    path: *const c_char,
    options_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let path = match c_str_to_string(path) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid path");
                return code;
            }
        };
        let options = match parse_upload_options(options_json) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid upload options");
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
                api.upload_image_from_path_with_progress(path, options, None)
                    .await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_upload_video(
    handle: FlareHandle,
    path: *const c_char,
    options_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let path = match c_str_to_string(path) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid path");
                return code;
            }
        };
        let options = match parse_upload_options(options_json) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid upload options");
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
                api.upload_video_from_path_with_progress(path, options, None)
                    .await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_upload_bytes(
    handle: FlareHandle,
    bytes: FlareBytesView,
    file_name: *const c_char,
    mime_type: *const c_char,
    options_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        if bytes.ptr.is_null() || bytes.len == 0 {
            let ctx = CallbackContext::new(context, callback);
            return_error(&ctx, FLARE_ERR_INVALID_PARAM, "Invalid bytes");
            return FLARE_ERR_INVALID_PARAM;
        }
        let payload = unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec();
        let file_name = match c_str_to_string(file_name) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid file_name");
                return code;
            }
        };
        let mime_type = match c_str_to_string(mime_type) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid mime_type");
                return code;
            }
        };
        let options = match parse_upload_options(options_json) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid upload options");
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
                api.upload_bytes(payload, file_name, mime_type, options)
                    .await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_delete_file(
    handle: FlareHandle,
    file_id: *const c_char,
    hard_delete: bool,
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
                api.delete_file(&file_id, hard_delete).await
            },
            |deleted| to_json_string(&serde_json::json!({ "deleted": deleted })),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_temp_download_url(
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
                api.get_temp_url_for_file_download(&file_id, expires_in)
                    .await
            },
            |v| to_json_string(&v),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_user_download_get_subfolder(
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
                api.user_download_get_subfolder().await
            },
            |subfolder| to_json_string(&serde_json::json!({ "subfolder": subfolder })),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_user_download_set_subfolder(
    handle: FlareHandle,
    name: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let name = match c_str_to_string(name) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid name");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async_unit(instance, ctx, async move {
            let api = inst.media_api().await?;
            api.user_download_set_subfolder(&name).await
        });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_user_download_get_saved_path(
    handle: FlareHandle,
    download_key: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let download_key = match c_str_to_string(download_key) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid download_key");
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
                api.user_download_get_saved_path(&download_key).await
            },
            |path| to_json_string(&serde_json::json!({ "path": path })),
        );
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_user_download_delete_record(
    handle: FlareHandle,
    download_key: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let download_key = match c_str_to_string(download_key) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid download_key");
                return code;
            }
        };
        let ctx = CallbackContext::new(context, callback);
        let inst = instance.clone();
        execute_async_unit(instance, ctx, async move {
            let api = inst.media_api().await?;
            api.user_download_delete_record(&download_key).await
        });
        0
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_cancel_user_file_download(
    handle: FlareHandle,
    download_key: *const c_char,
) -> bool {
    abi::catch_ffi_bool(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(_) => return false,
        };
        let download_key = match c_str_to_string(download_key) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let api = match instance
            .runtime
            .block_on(async { instance.media_api().await })
        {
            Ok(api) => api,
            Err(_) => return false,
        };
        api.cancel_user_file_download(&download_key)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_media_download_file_to_downloads(
    handle: FlareHandle,
    request_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let request: DownloadFileToDownloadsRequest = match abi::parse_json(request_json) {
            Ok(v) => v,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid download request JSON");
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
                api.download_file_to_user_downloads_folder(UserFileDownloadRequest {
                    download_key: request.download_key,
                    display_file_name: request.display_file_name,
                    source_path: request.source_path,
                    source_http_url: request.source_url,
                    remote_file_id: request.remote_file_id,
                    expires_in: request.expires_in.unwrap_or(3600),
                    on_progress: None,
                })
                .await
            },
            |path| to_json_string(&serde_json::json!({ "path": path })),
        );
        0
    })
}
