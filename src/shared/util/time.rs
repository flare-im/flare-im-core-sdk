//! Cross-platform monotonic-ish time helpers for WASM and native targets.

use std::future::Future;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_arch = "wasm32")]
use js_sys::Date;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

/// Wall-clock milliseconds since UNIX epoch.
pub fn now_millis() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[cfg(target_arch = "wasm32")]
    {
        Date::now() as u64
    }
}

pub fn deadline_after(duration: Duration) -> u64 {
    now_millis().saturating_add(duration.as_millis() as u64)
}

pub fn is_deadline_elapsed(deadline_ms: u64) -> bool {
    now_millis() >= deadline_ms
}

/// Async delay that works in browser WASM hosts (no Tokio timer reactor required).
#[cfg(target_arch = "wasm32")]
pub async fn delay(duration: Duration) {
    use std::cell::RefCell;
    use std::rc::Rc;

    use tokio::sync::oneshot;
    use wasm_bindgen::closure::Closure;

    let (tx, rx) = oneshot::channel::<()>();
    let millis = duration.as_millis().min(i32::MAX as u128) as i32;
    {
        let tx_slot = Rc::new(RefCell::new(Some(tx)));
        let tx_for_fallback = tx_slot.clone();
        let callback = Closure::once(Box::new(move || {
            if let Some(sender) = tx_slot.borrow_mut().take() {
                let _ = sender.send(());
            }
        }) as Box<dyn FnMut()>);
        let scheduled = web_sys::window().and_then(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.as_ref().unchecked_ref(),
                    millis,
                )
                .ok()
        });
        if scheduled.is_none()
            && let Some(sender) = tx_for_fallback.borrow_mut().take()
        {
            let _ = sender.send(());
        }
        callback.forget();
    }
    let _ = rx.await;
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn delay(duration: Duration) {
    tokio::time::sleep(duration).await;
}

/// Future timed out before completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutElapsed;

#[cfg(target_arch = "wasm32")]
pub async fn timeout<F, T>(duration: Duration, future: F) -> Result<T, TimeoutElapsed>
where
    F: Future<Output = T>,
{
    use futures::future::{Either, select};

    futures::pin_mut!(future);
    let sleeper = delay(duration);
    futures::pin_mut!(sleeper);
    match select(future, sleeper).await {
        Either::Left((value, _)) => Ok(value),
        Either::Right((_, _)) => Err(TimeoutElapsed),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn timeout<F, T>(duration: Duration, future: F) -> Result<T, TimeoutElapsed>
where
    F: Future<Output = T> + Send,
    T: Send,
{
    match tokio::time::timeout(duration, future).await {
        Ok(value) => Ok(value),
        Err(_) => Err(TimeoutElapsed),
    }
}
