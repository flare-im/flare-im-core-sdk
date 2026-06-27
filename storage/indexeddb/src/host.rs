//! JS IndexedDB host bridge for WASM production storage.

use std::cell::RefCell;
use std::rc::Rc;

use flare_im_core_sdk::model::Conversation;
use flare_im_core_sdk::model::IMMessage;
use flare_im_core_sdk::storage::PendingSendVo;
use flare_im_core_sdk::{FlareError, Result};
use js_sys::Function;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::Serializer;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

thread_local! {
    static STORAGE_HOST: RefCell<Option<Rc<StorageHost>>> = RefCell::new(None);
}

#[derive(Clone)]
struct StorageHost {
    load_snapshot: Function,
    save_message: Function,
    save_conversation: Function,
    save_cursor: Function,
    save_pending_send: Function,
    delete_message: Function,
    delete_conversation: Function,
    delete_pending_send: Function,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct SnapshotPayload {
    #[serde(default)]
    pub messages: Vec<IMMessage>,
    #[serde(default)]
    pub conversations: Vec<Conversation>,
    #[serde(default)]
    pub cursors: std::collections::HashMap<String, String>,
    #[serde(default, rename = "pendingSends")]
    pub pending_sends: Vec<PendingSendVo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistMessageArgs<'a> {
    user_id: &'a str,
    message: &'a IMMessage,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistConversationArgs<'a> {
    user_id: &'a str,
    conversation: &'a Conversation,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistCursorArgs<'a> {
    user_id: &'a str,
    key: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistPendingSendArgs<'a> {
    user_id: &'a str,
    entry: &'a PendingSendVo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteIdArgs<'a> {
    user_id: &'a str,
    id: &'a str,
}

async fn call_host_void(function: &Function, payload: &JsValue) -> Result<()> {
    let promise = function
        .call1(&JsValue::NULL, payload)
        .map_err(|e| FlareError::system(format!("storage host call failed: {e:?}")))?;
    if promise.is_instance_of::<js_sys::Promise>() {
        JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|e| FlareError::system(format!("storage host promise failed: {e:?}")))?;
    }
    Ok(())
}

async fn call_host_string(function: &Function, payload: &JsValue) -> Result<String> {
    let promise = function
        .call1(&JsValue::NULL, payload)
        .map_err(|e| FlareError::system(format!("storage host call failed: {e:?}")))?;
    let value = if promise.is_instance_of::<js_sys::Promise>() {
        JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|e| FlareError::system(format!("storage host promise failed: {e:?}")))?
    } else {
        promise
    };
    if value.is_null() || value.is_undefined() {
        return Ok(String::new());
    }
    value
        .as_string()
        .ok_or_else(|| FlareError::system("storage host returned non-string payload"))
}

fn to_host_value<T: Serialize>(payload: &T, operation: &str) -> Result<JsValue> {
    payload
        .serialize(&Serializer::json_compatible())
        .map_err(|e| FlareError::system(format!("encode {operation} payload failed: {e}")))
}

pub fn set_storage_host(
    load_snapshot: Function,
    save_message: Function,
    save_conversation: Function,
    save_cursor: Function,
    save_pending_send: Function,
    delete_message: Function,
    delete_conversation: Function,
    delete_pending_send: Function,
) {
    STORAGE_HOST.with(|slot| {
        *slot.borrow_mut() = Some(Rc::new(StorageHost {
            load_snapshot,
            save_message,
            save_conversation,
            save_cursor,
            save_pending_send,
            delete_message,
            delete_conversation,
            delete_pending_send,
        }));
    });
}

pub fn clear_storage_host() {
    STORAGE_HOST.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

pub fn storage_host_configured() -> bool {
    STORAGE_HOST.with(|slot| slot.borrow().is_some())
}

pub async fn load_snapshot(user_id: &str) -> Result<SnapshotPayload> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(SnapshotPayload::default());
    };
    let payload = to_host_value(&serde_json::json!({ "userId": user_id }), "storage load")?;
    let raw = call_host_string(&host.load_snapshot, &payload).await?;
    if raw.trim().is_empty() {
        return Ok(SnapshotPayload::default());
    }
    serde_json::from_str(&raw)
        .map_err(|e| FlareError::system(format!("decode storage snapshot failed: {e}")))
}

pub async fn persist_message(user_id: &str, message: &IMMessage) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(&PersistMessageArgs { user_id, message }, "persist message")?;
    call_host_void(&host.save_message, &payload).await
}

pub async fn persist_conversation(user_id: &str, conversation: &Conversation) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(
        &PersistConversationArgs {
            user_id,
            conversation,
        },
        "persist conversation",
    )?;
    call_host_void(&host.save_conversation, &payload).await
}

pub async fn persist_cursor(user_id: &str, key: &str, value: &str) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(
        &PersistCursorArgs {
            user_id,
            key,
            value,
        },
        "persist cursor",
    )?;
    call_host_void(&host.save_cursor, &payload).await
}

pub async fn persist_pending_send(user_id: &str, entry: &PendingSendVo) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(
        &PersistPendingSendArgs { user_id, entry },
        "persist pending send",
    )?;
    call_host_void(&host.save_pending_send, &payload).await
}

pub async fn delete_message(user_id: &str, message_id: &str) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(
        &DeleteIdArgs {
            user_id,
            id: message_id,
        },
        "delete message",
    )?;
    call_host_void(&host.delete_message, &payload).await
}

pub async fn delete_conversation(user_id: &str, conversation_id: &str) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(
        &DeleteIdArgs {
            user_id,
            id: conversation_id,
        },
        "delete conversation",
    )?;
    call_host_void(&host.delete_conversation, &payload).await
}

pub async fn delete_pending_send(user_id: &str, client_msg_id: &str) -> Result<()> {
    let host = STORAGE_HOST.with(|slot| slot.borrow().clone());
    let Some(host) = host else {
        return Ok(());
    };
    let payload = to_host_value(
        &DeleteIdArgs {
            user_id,
            id: client_msg_id,
        },
        "delete pending send",
    )?;
    call_host_void(&host.delete_pending_send, &payload).await
}
