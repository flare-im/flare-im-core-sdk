//! Flutter / 纯原生主线程等调用 FFI 时，线程上通常 **没有** Tokio runtime；
//! `Handle::try_current()` 会失败。此处提供进程级共享的 multi-thread runtime 供 `spawn` 使用。

use tokio::runtime::{Handle, Runtime};

lazy_static::lazy_static! {
    static ref FLARE_FFI_TOKIO: std::result::Result<Runtime, String> =
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("flare-ffi")
            .build()
            .map_err(|error| format!("flare_im_core_sdk_ffi: failed to create Tokio runtime: {error}"));
}

fn fallback_runtime() -> std::result::Result<&'static Runtime, String> {
    match &*FLARE_FFI_TOKIO {
        Ok(runtime) => Ok(runtime),
        Err(error) => Err(error.clone()),
    }
}

/// 供 `SdkInstance` 使用的 runtime handle：优先复用当前线程已存在的 runtime（如 `#[tokio::test]`），否则使用共享 runtime。
#[inline]
pub fn sdk_runtime_handle() -> std::result::Result<Handle, String> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return Ok(handle);
    }

    fallback_runtime().map(|runtime| runtime.handle().clone())
}
