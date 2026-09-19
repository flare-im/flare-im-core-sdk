//! SdkEvent -> JS callback for browser hosts.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use flare_im_core_sdk::SharedEventReceiver;
use flare_im_core_sdk::event::SdkEvent;
use js_sys::Function;
use serde::Serialize;
use tokio::sync::oneshot;
use wasm_bindgen::JsValue;

thread_local! {
    static EVENT_CALLBACKS: RefCell<HashMap<u64, Rc<Function>>> = RefCell::new(HashMap::new());
}

pub fn set_event_callback(runtime_id: u64, callback: Option<Function>) {
    EVENT_CALLBACKS.with(|slot| {
        let mut callbacks = slot.borrow_mut();
        if let Some(callback) = callback {
            callbacks.insert(runtime_id, Rc::new(callback));
        } else {
            callbacks.remove(&runtime_id);
        }
    });
}

pub fn clear_event_callback(runtime_id: u64) {
    set_event_callback(runtime_id, None);
}

pub fn emit_sdk_event_to_js(runtime_id: u64, ev: &SdkEvent) {
    let Some(payload) = flare_im_core_sdk_bindings_runtime::sdk_event_web_payload(ev) else {
        return;
    };
    // Release the RefCell borrow before calling JavaScript (callback may dispose).
    let callback = EVENT_CALLBACKS.with(|slot| slot.borrow().get(&runtime_id).cloned());
    if let Some(callback) = callback
        && let Ok(value) = payload.serialize(&serde_wasm_bindgen::Serializer::json_compatible())
    {
        let _ = callback.call1(&JsValue::NULL, &value);
    }
}

pub async fn forward_event_rx_to_js(
    runtime_id: u64,
    mut rx: SharedEventReceiver,
    mut cancel_rx: oneshot::Receiver<()>,
) {
    loop {
        tokio::select! {
            _ = &mut cancel_rx => {
                break;
            },
            result = rx.recv() => {
                let ev = match result {
                    Ok(event) => event,
                    Err(_) => {
                        break;
                    },
                };
                emit_sdk_event_to_js(runtime_id, ev.event());
            }
        }
    }
}
