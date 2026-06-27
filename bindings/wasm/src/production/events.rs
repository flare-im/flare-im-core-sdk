//! SdkEvent -> JS callback for browser hosts.

use std::cell::RefCell;
use std::rc::Rc;

use flare_im_core_sdk::SharedEventReceiver;
use flare_im_core_sdk::event::SdkEvent;
use js_sys::Function;
use serde::Serialize;
use tokio::sync::oneshot;
use wasm_bindgen::JsValue;

thread_local! {
    static EVENT_CALLBACK: RefCell<Option<Rc<Function>>> = RefCell::new(None);
}

pub fn set_event_callback(callback: Option<Function>) {
    EVENT_CALLBACK.with(|slot| {
        *slot.borrow_mut() = callback.map(Rc::new);
    });
}

pub fn clear_event_callback() {
    set_event_callback(None);
}

pub fn emit_sdk_event_to_js(ev: &SdkEvent) {
    let Some(payload) = flare_im_core_sdk_bindings_runtime::sdk_event_web_payload(ev) else {
        return;
    };
    EVENT_CALLBACK.with(|slot| {
        let Some(callback) = slot.borrow().clone() else {
            return;
        };
        if let Ok(value) = payload.serialize(&serde_wasm_bindgen::Serializer::json_compatible()) {
            let _ = callback.call1(&JsValue::NULL, &value);
        }
    });
}

pub async fn forward_event_rx_to_js(
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
                emit_sdk_event_to_js(ev.event());
            }
        }
    }
}
