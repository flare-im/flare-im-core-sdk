//! Re-export shared WASM Tokio driver from `flare-core`.

#[cfg(target_arch = "wasm32")]
pub use flare_core::client::wasm_tokio::{
    ensure_initialized, run_async as run_sdk, spawn_detached,
};

#[cfg(not(target_arch = "wasm32"))]
pub fn ensure_initialized() {}

#[cfg(not(target_arch = "wasm32"))]
pub async fn run_sdk<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    future.await
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_detached<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        let _ = tokio::spawn(future);
        return;
    }
    let _ = std::thread::Builder::new()
        .name("flare-wasm-host-test-task".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(future);
        });
}
