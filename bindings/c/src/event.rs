//! 事件 API - 订阅与转发（payload 序列化见共享 runtime event helpers）

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::abi;
use crate::generated::events::FLARE_EVENT_UNKNOWN;
use crate::helpers::string_to_flare;
use crate::registry::{SdkInstance, require_instance};
use crate::types::{
    FlareEventBatchCallback, FlareEventCallback, FlareHandle, FlareSubscriptionHandle,
};
use dashmap::DashMap;
use flare_im_core_sdk::{RawSdkEvent, SharedEventReceiver};
use flare_im_core_sdk_bindings_runtime::{sdk_event_batch_json, sdk_event_code, sdk_event_json};
use tokio::sync::{mpsc::error::TryRecvError, oneshot};
use tokio::time::{Duration, sleep};

const EVENT_BATCH_MAX_EVENTS: usize = 128;

lazy_static::lazy_static! {
    static ref EVENT_SUBSCRIPTIONS: DashMap<u64, EventSubscription> = DashMap::new();
}

struct EventSubscription {
    handle: FlareHandle,
    cancel: oneshot::Sender<()>,
    state: Arc<EventSubscriptionState>,
}

#[derive(Clone, Copy)]
enum EventDelivery {
    Single(FlareEventCallback),
    Batch(FlareEventBatchCallback),
}

struct EventSubscriptionState {
    active: AtomicBool,
    in_flight_callbacks: Mutex<usize>,
    idle: Condvar,
}

struct EventCallbackGuard {
    state: Arc<EventSubscriptionState>,
}

struct CallbackThreadScope {
    previous_subscription_id: u64,
}

thread_local! {
    static CURRENT_EVENT_CALLBACK_SUBSCRIPTION: Cell<u64> = const { Cell::new(0) };
}

impl EventSubscriptionState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            active: AtomicBool::new(true),
            in_flight_callbacks: Mutex::new(0),
            idle: Condvar::new(),
        })
    }

    fn stop(&self) {
        self.active.store(false, Ordering::Release);
        self.idle.notify_all();
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn begin_callback(self: &Arc<Self>) -> Option<EventCallbackGuard> {
        if !self.is_active() {
            return None;
        }
        let mut in_flight = self
            .in_flight_callbacks
            .lock()
            .expect("event callback counter poisoned");
        if !self.is_active() {
            return None;
        }
        *in_flight += 1;
        Some(EventCallbackGuard {
            state: Arc::clone(self),
        })
    }

    fn wait_for_idle(&self, subscription_id: u64) {
        let called_from_this_callback =
            CURRENT_EVENT_CALLBACK_SUBSCRIPTION.with(|current| current.get() == subscription_id);
        if called_from_this_callback {
            return;
        }

        let mut in_flight = self
            .in_flight_callbacks
            .lock()
            .expect("event callback counter poisoned");
        while *in_flight > 0 {
            in_flight = self
                .idle
                .wait(in_flight)
                .expect("event callback counter poisoned");
        }
    }
}

impl Drop for EventCallbackGuard {
    fn drop(&mut self) {
        let mut in_flight = self
            .state
            .in_flight_callbacks
            .lock()
            .expect("event callback counter poisoned");
        *in_flight = in_flight.saturating_sub(1);
        if *in_flight == 0 {
            self.state.idle.notify_all();
        }
    }
}

impl CallbackThreadScope {
    fn enter(subscription_id: u64) -> Self {
        let previous_subscription_id =
            CURRENT_EVENT_CALLBACK_SUBSCRIPTION.with(|current| current.replace(subscription_id));
        Self {
            previous_subscription_id,
        }
    }
}

impl Drop for CallbackThreadScope {
    fn drop(&mut self) {
        CURRENT_EVENT_CALLBACK_SUBSCRIPTION
            .with(|current| current.set(self.previous_subscription_id));
    }
}

fn cancel_subscription(subscription_id: u64, subscription: EventSubscription) {
    subscription.state.stop();
    let _ = subscription.cancel.send(());
    subscription.state.wait_for_idle(subscription_id);
}

pub(crate) fn unsubscribe_all_events() {
    let keys: Vec<u64> = EVENT_SUBSCRIPTIONS.iter().map(|e| *e.key()).collect();
    for key in keys {
        if let Some((_, sub)) = EVENT_SUBSCRIPTIONS.remove(&key) {
            cancel_subscription(key, sub);
        }
    }
}

pub(crate) fn unsubscribe_events_for_handle(handle: FlareHandle) {
    let keys: Vec<u64> = EVENT_SUBSCRIPTIONS
        .iter()
        .filter_map(|e| (e.value().handle == handle).then_some(*e.key()))
        .collect();
    for key in keys {
        if let Some((_, sub)) = EVENT_SUBSCRIPTIONS.remove(&key) {
            cancel_subscription(key, sub);
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
        subscribe_events_inner(handle, context, EventDelivery::Single(callback)).unwrap_or_default()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_event_subscribe_batch(
    handle: FlareHandle,
    context: *mut c_void,
    callback: FlareEventBatchCallback,
) -> FlareSubscriptionHandle {
    abi::catch_ffi_subscription_handle(|| {
        subscribe_events_inner(handle, context, EventDelivery::Batch(callback)).unwrap_or_default()
    })
}

fn subscribe_events_inner(
    handle: FlareHandle,
    context: *mut c_void,
    delivery: EventDelivery,
) -> Result<FlareSubscriptionHandle, i32> {
    let instance = require_instance(handle)?;

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let state = EventSubscriptionState::new();

    let subscription_id = {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_SUB_ID: AtomicU64 = AtomicU64::new(1);
        NEXT_SUB_ID.fetch_add(1, Ordering::SeqCst)
    };
    EVENT_SUBSCRIPTIONS.insert(
        subscription_id,
        EventSubscription {
            handle,
            cancel: cancel_tx,
            state: Arc::clone(&state),
        },
    );

    spawn_event_forwarder(
        instance,
        subscription_id,
        context as usize,
        delivery,
        state,
        cancel_rx,
    );

    Ok(subscription_id)
}

fn spawn_event_forwarder(
    instance: Arc<SdkInstance>,
    subscription_id: u64,
    user_context: usize,
    delivery: EventDelivery,
    state: Arc<EventSubscriptionState>,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let client = instance.client.clone();

    instance.runtime.spawn(async move {
        let mut rx = loop {
            match client.bus().await {
                Ok(bus) => break bus.subscribe_shared_raw(),
                Err(e) => {
                    tracing::warn!(error = %e, "event bus not ready yet, retrying");
                    tokio::select! {
                        _ = &mut cancel_rx => {
                            tracing::debug!("Event subscription cancelled before bus ready");
                            state.stop();
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
                    state.stop();
                    EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                    break;
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            if !state.is_active() {
                                break;
                            }
                            match delivery {
                                EventDelivery::Single(callback) => {
                                    emit_single_event(
                                        subscription_id,
                                        &state,
                                        callback,
                                        user_context,
                                        event.as_ref(),
                                    );
                                }
                                EventDelivery::Batch(callback) => {
                                    let batch = drain_event_batch(&mut rx, event);
                                    emit_event_batch(
                                        subscription_id,
                                        &state,
                                        callback,
                                        user_context,
                                        &batch,
                                    );
                                }
                            }
                        }
                        Err(_) => {
                            tracing::debug!("Event bus closed");
                            state.stop();
                            EVENT_SUBSCRIPTIONS.remove(&subscription_id);
                            break;
                        }
                    }
                }
            }
        }
    });
}

fn drain_event_batch(
    rx: &mut SharedEventReceiver,
    first: Arc<RawSdkEvent>,
) -> Vec<Arc<RawSdkEvent>> {
    let mut batch = Vec::with_capacity(EVENT_BATCH_MAX_EVENTS.min(16));
    batch.push(first);

    while batch.len() < EVENT_BATCH_MAX_EVENTS {
        match rx.try_recv() {
            Ok(event) => batch.push(event),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }

    batch
}

fn emit_single_event(
    subscription_id: u64,
    state: &Arc<EventSubscriptionState>,
    callback: FlareEventCallback,
    user_context: usize,
    event: &RawSdkEvent,
) {
    let Some(_callback_guard) = state.begin_callback() else {
        return;
    };
    let event_type = sdk_event_code(event.event());
    if event_type == FLARE_EVENT_UNKNOWN {
        return;
    }

    let event_json = event.cached_json(sdk_event_json);
    let event_json = string_to_flare(event_json.as_ref().to_owned());

    abi::invoke_user_c_callback("FlareEventCallback", || {
        let _scope = CallbackThreadScope::enter(subscription_id);
        callback(user_context as *mut c_void, event_type, event_json);
    });
}

fn emit_event_batch(
    subscription_id: u64,
    state: &Arc<EventSubscriptionState>,
    callback: FlareEventBatchCallback,
    user_context: usize,
    batch: &[Arc<RawSdkEvent>],
) {
    let Some(_callback_guard) = state.begin_callback() else {
        return;
    };
    let Some((events_json, event_count)) =
        sdk_event_batch_json(batch.iter().map(|event| event.as_ref()))
    else {
        return;
    };
    let events_json = string_to_flare(events_json);

    abi::invoke_user_c_callback("FlareEventBatchCallback", || {
        let _scope = CallbackThreadScope::enter(subscription_id);
        callback(user_context as *mut c_void, event_count, events_json);
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe(subscription: FlareSubscriptionHandle) {
    abi::catch_ffi_void(|| {
        if let Some((_, sub)) = EVENT_SUBSCRIPTIONS.remove(&subscription) {
            cancel_subscription(subscription, sub);
        } else {
            tracing::debug!("Unsubscribe {} ignored: not found", subscription);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn flare_event_unsubscribe_all() {
    abi::catch_ffi_void(unsubscribe_all_events);
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_im_core_sdk::EventBus;
    use flare_im_core_sdk::event::{ConversationEvent, SdkEvent};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn drain_event_batch_caps_immediate_burst() {
        let bus = EventBus::with_capacity(EVENT_BATCH_MAX_EVENTS + 8);
        let mut rx = bus.subscribe_shared_raw();

        for index in 0..(EVENT_BATCH_MAX_EVENTS + 2) {
            bus.publish(SdkEvent::Conversation(ConversationEvent::Created {
                conversation_id: format!("conversation-{index}"),
            }));
        }

        let first = rx.try_recv().expect("first event");
        let batch = drain_event_batch(&mut rx, first);

        assert_eq!(batch.len(), EVENT_BATCH_MAX_EVENTS);
        assert!(
            rx.try_recv().is_ok(),
            "events past the batch cap remain queued for the next callback"
        );
    }

    #[test]
    fn event_subscription_state_rejects_callbacks_after_stop() {
        let state = EventSubscriptionState::new();

        assert!(state.begin_callback().is_some());
        state.stop();
        assert!(state.begin_callback().is_none());
    }

    #[test]
    fn event_subscription_state_waits_for_in_flight_callbacks() {
        let state = EventSubscriptionState::new();
        let guard = state
            .begin_callback()
            .expect("callback should start while subscription is active");
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        thread::spawn(move || {
            release_rx.recv().expect("wait for callback release");
            drop(guard);
        });

        let waiter_state = Arc::clone(&state);
        thread::spawn(move || {
            waiter_state.stop();
            waiter_state.wait_for_idle(42);
            done_tx.send(()).expect("send waiter completion");
        });

        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "unsubscribe must not complete while a callback is in flight"
        );

        release_tx.send(()).expect("release callback");
        done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("unsubscribe completes after callback returns");
    }
}
