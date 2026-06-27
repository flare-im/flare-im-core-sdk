//! Detached background tasks — native Tokio runtime vs browser `spawn_local`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

/// Abort handle for long-lived SDK background workers.
pub struct BackgroundTask {
    cancel: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    abort: Option<tokio::task::AbortHandle>,
}

impl BackgroundTask {
    pub fn abort(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(abort) = &self.abort {
            abort.abort();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct CancellableFuture {
    inner: Pin<Box<dyn Future<Output = ()> + Send>>,
    cancel: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Future for CancellableFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        if this.cancel.load(Ordering::Relaxed) {
            return Poll::Ready(());
        }
        match this.inner.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(()),
            Poll::Pending => {
                if this.cancel.load(Ordering::Relaxed) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wrap_cancellable<F>(future: F, cancel: Arc<AtomicBool>) -> CancellableFuture
where
    F: Future<Output = ()> + Send + 'static,
{
    CancellableFuture {
        inner: Box::pin(future),
        cancel,
    }
}

#[cfg(target_arch = "wasm32")]
struct WasmCancellableFuture {
    inner: Pin<Box<dyn Future<Output = ()>>>,
    cancel: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
impl Future for WasmCancellableFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        if this.cancel.load(Ordering::Relaxed) {
            return Poll::Ready(());
        }
        match this.inner.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(()),
            Poll::Pending => {
                if this.cancel.load(Ordering::Relaxed) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn wrap_cancellable<F>(future: F, cancel: Arc<AtomicBool>) -> WasmCancellableFuture
where
    F: Future<Output = ()> + 'static,
{
    WasmCancellableFuture {
        inner: Box::pin(future),
        cancel,
    }
}

/// Spawn a fire-and-forget background task.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::mem::drop(tokio::spawn(future));
        return;
    }
    let _ = thread::Builder::new()
        .name("flare-sdk-background".into())
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

/// Spawn a fire-and-forget background task.
#[cfg(target_arch = "wasm32")]
pub fn spawn_background<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    spawn_local(future);
}

/// Spawn an abortable background worker.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_background_task<F>(future: F) -> BackgroundTask
where
    F: Future<Output = ()> + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let wrapped = wrap_cancellable(future, cancel.clone());
    let abort = if tokio::runtime::Handle::try_current().is_ok() {
        let join = tokio::spawn(wrapped);
        let abort = join.abort_handle();
        drop(join);
        Some(abort)
    } else {
        let (tx, rx) = mpsc::sync_channel(1);
        match thread::Builder::new()
            .name("flare-sdk-background-task".into())
            .spawn(move || {
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    let _ = tx.send(None);
                    return;
                };
                let join = rt.spawn(wrapped);
                let abort = join.abort_handle();
                let _ = tx.send(Some(abort));
                let _ = rt.block_on(join);
            }) {
            Ok(_) => match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(abort) => abort,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "background task thread did not return abort handle"
                    );
                    None
                }
            },
            Err(error) => {
                tracing::warn!(error = %error, "spawn background task thread failed");
                None
            }
        }
    };
    BackgroundTask { cancel, abort }
}

/// Spawn an abortable background worker.
#[cfg(target_arch = "wasm32")]
pub fn spawn_background_task<F>(future: F) -> BackgroundTask
where
    F: Future<Output = ()> + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let wrapped = wrap_cancellable(future, cancel.clone());
    spawn_local(wrapped);
    BackgroundTask { cancel }
}
