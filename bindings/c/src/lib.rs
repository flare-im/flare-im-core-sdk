//! C ABI for Flare IM Core SDK vNext.
//!
//! The ABI is deliberately small: platform SDKs hold an opaque handle and use
//! JSON contract routes. All IM behavior stays inside `flare-im-core-sdk`.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use flare_im_core_sdk::IMClient;

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static CLIENTS: OnceLock<Mutex<HashMap<u64, IMClient>>> = OnceLock::new();
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn clients() -> &'static Mutex<HashMap<u64, IMClient>> {
    CLIENTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("flare-im-c-ffi")
            .build()
            .expect("create flare im ffi runtime")
    })
}

fn c_str(ptr: *const c_char) -> Result<String, String> {
    if ptr.is_null() {
        return Err("null string pointer".to_string());
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| e.to_string())?;
    Ok(s.to_string())
}

fn into_c_string(value: impl Into<String>) -> *mut c_char {
    let sanitized = value.into().replace('\0', "\\u0000");
    CString::new(sanitized)
        .expect("sanitized string has no interior nul")
        .into_raw()
}

fn error_json(message: impl Into<String>) -> *mut c_char {
    into_c_string(
        serde_json::json!({
            "ok": false,
            "data": null,
            "error": {
                "code": "FfiError",
                "message": message.into()
            }
        })
        .to_string(),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_version() -> *mut c_char {
    into_c_string(env!("CARGO_PKG_VERSION"))
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_binding_contract_version() -> *mut c_char {
    into_c_string(flare_im_core_sdk_bindings_runtime::BINDING_CONTRACT_VERSION)
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_binding_contract_json() -> *mut c_char {
    into_c_string(flare_im_core_sdk_bindings_runtime::contract_json())
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_client_init_example_json() -> *mut c_char {
    into_c_string(flare_im_core_sdk_bindings_runtime::client_init_request_example_json())
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_generate_test_token(user_id: *const c_char) -> *mut c_char {
    match c_str(user_id) {
        Ok(user_id) => into_c_string(flare_im_core_sdk::generate_test_token(&user_id)),
        Err(error) => error_json(error),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_create(config_json: *const c_char) -> u64 {
    let Ok(config_json) = c_str(config_json) else {
        return 0;
    };
    let Ok(client) = IMClient::from_config_json(&config_json) else {
        return 0;
    };
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut guard) = clients().lock() {
        guard.insert(handle, client);
        handle
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_release(handle: u64) {
    if let Ok(mut guard) = clients().lock() {
        guard.remove(&handle);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_invoke_json(handle: u64, request_json: *const c_char) -> *mut c_char {
    let request_json = match c_str(request_json) {
        Ok(value) => value,
        Err(error) => return error_json(error),
    };
    let client = match clients().lock() {
        Ok(guard) => guard.get(&handle).cloned(),
        Err(_) => return error_json("client registry lock poisoned"),
    };
    let Some(client) = client else {
        return error_json(format!("invalid sdk handle: {handle}"));
    };
    let response = runtime().block_on(async {
        flare_im_core_sdk_bindings_runtime::invoke_json(&client, &request_json).await
    });
    into_c_string(response)
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_sdk_init_logging() {
    let _ = tracing_subscriber::fmt::try_init();
}
