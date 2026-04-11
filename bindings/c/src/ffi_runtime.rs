//! Flutter / 纯原生主线程等调用 FFI 时，线程上通常 **没有** Tokio runtime；
//! `Handle::try_current()` 会失败。此处提供进程级共享的 multi-thread runtime 供 `spawn` 使用。

use tokio::runtime::{Handle, Runtime};

lazy_static::lazy_static! {
    static ref FLARE_FFI_TOKIO: Runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("flare-ffi")
        .build()
        .expect("flare_im_core_sdk_ffi: failed to create Tokio runtime");
}

/// 供 `SdkInstance` 使用的 runtime handle：优先复用当前线程已存在的 runtime（如 `#[tokio::test]`），否则使用共享 runtime。
#[inline]
pub fn sdk_runtime_handle() -> Handle {
    tokio::runtime::Handle::try_current().unwrap_or_else(|_| FLARE_FFI_TOKIO.handle().clone())
}
