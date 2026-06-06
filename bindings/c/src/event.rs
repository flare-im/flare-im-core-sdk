//! 事件 API - 订阅与转发（payload 序列化见 `bindings/runtime/event_bridge`）

use std::ffi::c_void;
use std::sync::Arc;

use crate::abi;
use crate::generated::events::FLARE_EVENT_UNKNOWN;
use crate::helpers::string_to_flare;
use crate::registry::{SdkInstance, require_instance};
use crate::types::{FlareEventCallback, FlareHandle, FlareSubscriptionHandle};
use dashmap::DashMap;
use flare_im_core_sdk_bindings_runtime::{sdk_event_code, sdk_event_json};
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};

lazy_static::lazy_static! {
    static ref EVENT_SUBSCRIPTIONS: DashMap<u64, oneshot::Sender<()>> = DashMap::new();
}

pub(crate) fn unsubscribe_all_events() {
    let keys: Vec<u64> = EVENT_SUBSCRIPTIONS.iter().map(|e| *e.key()).collect();
    for key in keys {
        if let Some((_, tx)) = EVENT_SUBSCRIPTIONS.remove(&key) {
            let _ = tx.send(());
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_event_subscribe(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> FlareSubscriptionHandle {
    abi::catch_ffi_subscription_handle(|| {
        subscribe_events_inner(handle, context, callback).unwrap_or_default()
    })
}

fn subscribe_events_inner(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> Result<FlareSubscriptionHandle, i32> {
    let instance = require_instance(handle)?;

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();

    let subscription_id = {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_SUB_ID: AtomicU64 = AtomicU64::new(1);
        NEXT_SUB_ID.fetch_add(1, Ordering::SeqCst)
    };
    EVENT_SUBSCRIPTIONS.insert(subscription_id, cancel_tx);

    spawn_event_forwarder(
        instance,
        subscription_id,
        context as usize,
        callback,
        cancel_rx,
    );

    Ok(subscription_id)
}

fn spawn_event_forwarder(
    instance: Arc<SdkInstance>,
    subscription_id: u64,
    user_context: usize,
    callback: FlareEventCallback,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let mut rx = loop {
            match client.bus().await {
                Ok(bus) => break bus.subscribe(),
                Err(e) => {
                    tracing::warn!(error = %e, "event bus not ready yet, retrying");
                    tokio::select! {
                        _ = &mut cancel_rx => {
                            tracing::debug!("Event subscription cancelled before bus ready");
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            return;
                        }
                        _ = sleep(Duration::from_millis(200)) => {}
                    }
                }
            }
        };

        loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    tracing::debug!("Event subscription cancelled");
                    EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                    break;
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let event_type = sdk_event_code(&event);
                            if event_type == FLARE_EVENT_UNKNOWN {
                                continue;
                            }
                            let event_json = string_to_flare(sdk_event_json(&event));

                            abi::invoke_user_c_callback("FlareEventCallback", || {
                                callback(user_context as *mut c_void, event_type, event_json);
                            });
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::debug!("Event bus closed");
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Event subscription lagged, missed {} events", n);
                        }
                    }
                }
            }
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe(subscription: FlareSubscriptionHandle) {
    abi::catch_ffi_void(|| {
        if let Some((_, tx)) = EVENT_SUBSCRIPTIONS.remove(&subscription) {
            let _ = tx.send(());
        } else {
            tracing::debug!("Unsubscribe {} ignored: not found", subscription);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe_all() {
    abi::catch_ffi_void(unsubscribe_all_events);
}
