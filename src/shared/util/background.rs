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
    finished: Arc<AtomicBool>,
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

    /// Whether the worker has completed (ran to completion or was aborted).
    ///
    /// Finished handles can be dropped by holders to avoid unbounded growth.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed) || self.cancel.load(Ordering::Relaxed)
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct CancellableFuture {
    inner: Pin<Box<dyn Future<Output = ()> + Send>>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Future for CancellableFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        if this.cancel.load(Ordering::Relaxed) {
            this.finished.store(true, Ordering::Relaxed);
            return Poll::Ready(());
        }
        match this.inner.as_mut().poll(cx) {
            Poll::Ready(()) => {
                this.finished.store(true, Ordering::Relaxed);
                Poll::Ready(())
            }
            Poll::Pending => {
                if this.cancel.load(Ordering::Relaxed) {
                    this.finished.store(true, Ordering::Relaxed);
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn wrap_cancellable<F>(
    future: F,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
) -> CancellableFuture
where
    F: Future<Output = ()> + Send + 'static,
{
    CancellableFuture {
        inner: Box::pin(future),
        cancel,
        finished,
    }
}

#[cfg(target_arch = "wasm32")]
struct WasmCancellableFuture {
    inner: Pin<Box<dyn Future<Output = ()>>>,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
}

#[cfg(target_arch = "wasm32")]
impl Future for WasmCancellableFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.as_mut().get_mut();
        if this.cancel.load(Ordering::Relaxed) {
            this.finished.store(true, Ordering::Relaxed);
            return Poll::Ready(());
        }
        match this.inner.as_mut().poll(cx) {
            Poll::Ready(()) => {
                this.finished.store(true, Ordering::Relaxed);
                Poll::Ready(())
            }
            Poll::Pending => {
                if this.cancel.load(Ordering::Relaxed) {
                    this.finished.store(true, Ordering::Relaxed);
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn wrap_cancellable<F>(
    future: F,
    cancel: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
) -> WasmCancellableFuture
where
    F: Future<Output = ()> + 'static,
{
    WasmCancellableFuture {
        inner: Box::pin(future),
        cancel,
        finished,
    }
}

/// 无环境 runtime 时共用的进程级后台 runtime。
///
/// 原生 FFI 宿主（iOS/Android 同步桥）从非 tokio 线程调入时，FTS backfill、waterline 补拉等
/// fire-and-forget 都走这条 fallback：**每次起一个 OS 线程 + current_thread runtime** 会在
/// 高频路径上把线程当一次性资源用。共享一个小型多线程 runtime 后，任务由其 worker 直接驱动。
///
/// 静态 `OnceLock` 持有，进程存续期不 drop（drop `Runtime` 会等待在跑的任务）。
#[cfg(not(target_arch = "wasm32"))]
fn fallback_runtime() -> Option<&'static tokio::runtime::Runtime> {
    use std::sync::OnceLock;
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("flare-sdk-background")
                .enable_all()
                .build()
                .map_err(|error| {
                    tracing::warn!(error = %error, "build shared background runtime failed");
                })
                .ok()
        })
        .as_ref()
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
    let Some(runtime) = fallback_runtime() else {
        return;
    };
    std::mem::drop(runtime.spawn(future));
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
    let finished = Arc::new(AtomicBool::new(false));
    let wrapped = wrap_cancellable(future, cancel.clone(), finished.clone());
    let abort = if tokio::runtime::Handle::try_current().is_ok() {
        let join = tokio::spawn(wrapped);
        let abort = join.abort_handle();
        drop(join);
        Some(abort)
    } else if let Some(runtime) = fallback_runtime() {
        // 共享 runtime 直接给出 abort handle：不再每任务起线程，也省掉回传 handle 的
        // 通道 + 1s 超时（超时曾让 abort 静默失效）。
        let join = runtime.spawn(wrapped);
        let abort = join.abort_handle();
        drop(join);
        Some(abort)
    } else {
        None
    };
    BackgroundTask {
        cancel,
        finished,
        abort,
    }
}

/// Spawn an abortable background worker.
#[cfg(target_arch = "wasm32")]
pub fn spawn_background_task<F>(future: F) -> BackgroundTask
where
    F: Future<Output = ()> + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let wrapped = wrap_cancellable(future, cancel.clone(), finished.clone());
    spawn_local(wrapped);
    BackgroundTask { cancel, finished }
}
