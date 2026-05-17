//! JSON 分发入口：覆盖 [`MessageApi`] / [`MessageBuildApi`] 中其余未单独导出的 C 符号，与 `src/client` Facade 对齐。
//!
//! - [`flare_message_dispatch_json`]：`op` + `params_json`（对象）
//! - [`flare_message_build_json`]：请求 JSON 须含字符串字段 `"op"`，其余字段随 `MessageBuildApi::create_*` 而定

use std::ffi::{c_char, c_void};

use crate::abi;
use crate::error_convert::{FLARE_ERR_INVALID_PARAM, FLARE_ERR_JSON_PARSE};
use crate::executor::{CallbackContext, execute_async, execute_async_unit, return_error};
use crate::helpers::{c_str_to_string, parse_json, to_json_string};
use crate::registry::require_instance;
use crate::types::{FlareHandle, FlareResultCallback};

use flare_im_core_sdk::model::content_builder::BuiltContent;
use flare_im_core_sdk::model::message::{IMMessage, MarkType, SendAck};
use flare_im_core_sdk::model::message_elem::{Elem, elem_to_message_content};
use flare_proto::common::{CallMediaType, MessageType};
use serde::Deserialize;

fn parse_mark_type(v: i32) -> MarkType {
    match v {
        1 => MarkType::Important,
        2 => MarkType::Todo,
        3 => MarkType::Done,
        _ => MarkType::Custom,
    }
}

/// `flare_message_send` 与 `dispatch_json` 的 `send` 共用，须为合法 JSON（供 Dart `jsonDecode`）。
pub(crate) fn send_ack_to_json(ack: &SendAck) -> Result<String, i32> {
    to_json_string(&serde_json::json!({
        "client_msg_id": ack.client_msg_id,
        "server_msg_id": ack.server_msg_id,
        "seq": ack.seq,
        "conversation_id": ack.conversation_id,
        "success": ack.success,
        "error_code": ack.error_code,
        "error_message": ack.error_message,
    }))
}

fn json_str<'a>(v: &'a serde_json::Value, key: &str) -> Result<&'a str, ()> {
    v.get(key).and_then(|x| x.as_str()).ok_or(())
}

fn json_u64(v: &serde_json::Value, key: &str) -> Result<u64, ()> {
    v.get(key)
        .and_then(|x| x.as_u64().or_else(|| x.as_i64().map(|i| i as u64)))
        .ok_or(())
}

fn json_i32(v: &serde_json::Value, key: &str) -> Result<i32, ()> {
    v.get(key)
        .and_then(|x| x.as_i64().map(|i| i as i32))
        .ok_or(())
}

fn json_bool(v: &serde_json::Value, key: &str) -> Result<bool, ()> {
    v.get(key).and_then(|x| x.as_bool()).ok_or(())
}

fn json_vec_str(v: &serde_json::Value, key: &str) -> Result<Vec<String>, ()> {
    let arr = v.get(key).and_then(|x| x.as_array()).ok_or(())?;
    let mut out = Vec::with_capacity(arr.len());
    for e in arr {
        out.push(e.as_str().ok_or(())?.to_string());
    }
    Ok(out)
}

fn json_vec_message(v: &serde_json::Value, key: &str) -> Result<Vec<IMMessage>, i32> {
    let arr = v
        .get(key)
        .and_then(|x| x.as_array())
        .ok_or(FLARE_ERR_INVALID_PARAM)?;
    let mut out = Vec::with_capacity(arr.len());
    for e in arr {
        let m: IMMessage = serde_json::from_value(e.clone()).map_err(|_| FLARE_ERR_JSON_PARSE)?;
        out.push(m);
    }
    Ok(out)
}

/// JSON：`{ "message_type": <i32>, "content": <Elem 对象> }`，与 Tauri / IMMessage 展示模型一致。
#[derive(Deserialize)]
struct BuiltContentJsonShell {
    message_type: i32,
    content: Elem,
}

fn built_content_from_value(v: &serde_json::Value) -> Result<BuiltContent, i32> {
    let shell: BuiltContentJsonShell =
        serde_json::from_value(v.clone()).map_err(|_| FLARE_ERR_JSON_PARSE)?;
    let mt = MessageType::try_from(shell.message_type).unwrap_or(MessageType::Unspecified);
    let inner = elem_to_message_content(&shell.content);
    Ok(BuiltContent::new(mt, inner))
}

/// `op`：与 [`MessageApi`] 方法名对应的 snake_case（如 `get`、`send`、`search`）
#[unsafe(no_mangle)]
pub extern "C" fn flare_message_dispatch_json(
    handle: FlareHandle,
    op: *const c_char,
    params_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let op = match c_str_to_string(op) {
            Ok(s) => s,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid op");
                return code;
            }
        };
        let params: serde_json::Value = match parse_json(params_json) {
            Ok(p) => p,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid params JSON");
                return code;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();

        macro_rules! bad_param {
            () => {{
                return_error(
                    &ctx,
                    FLARE_ERR_INVALID_PARAM,
                    "missing or invalid JSON field",
                );
                return FLARE_ERR_INVALID_PARAM;
            }};
        }

        match op.as_str() {
            "get" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.message()?;
                        api.get(&id).await
                    },
                    |opt| match opt {
                        Some(m) => to_json_string(&m),
                        None => Ok("null".to_string()),
                    },
                );
            }
            "get_raw" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.message()?;
                        api.get_raw(&id).await
                    },
                    |opt| match opt {
                        Some(m) => to_json_string(&m),
                        None => Ok("null".to_string()),
                    },
                );
            }
            "search" => {
                let kw = match json_str(&params, "keyword") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let limit = json_i32(&params, "limit").unwrap_or(50).max(1) as u32;
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.message()?;
                        api.search(&kw, limit).await
                    },
                    |list| to_json_string(&list),
                );
            }
            "send" => {
                let msg: IMMessage = match serde_json::from_value(params.clone()) {
                    Ok(m) => m,
                    Err(_) => match params.get("message") {
                        Some(mv) => match serde_json::from_value(mv.clone()) {
                            Ok(m) => m,
                            Err(_) => bad_param!(),
                        },
                        None => bad_param!(),
                    },
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.message()?;
                        api.send(msg).await
                    },
                    |ack| send_ack_to_json(&ack),
                );
            }
            "send_no_oss" => {
                let msg: IMMessage = match serde_json::from_value(params.clone()) {
                    Ok(m) => m,
                    Err(_) => match params.get("message") {
                        Some(mv) => match serde_json::from_value(mv.clone()) {
                            Ok(m) => m,
                            Err(_) => bad_param!(),
                        },
                        None => bad_param!(),
                    },
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.message()?;
                        api.send_no_oss(msg).await
                    },
                    |ack| send_ack_to_json(&ack),
                );
            }
            "recall" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.recall(&id).await
                });
            }
            "delete" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.delete(&id).await
                });
            }
            "delete_for_self" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let reason = params
                    .get("reason")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.delete_for_self(&id, reason).await
                });
            }
            "delete_for_everyone" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let reason = params
                    .get("reason")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.delete_for_everyone(&id, reason).await
                });
            }
            "edit_text_by_message_id" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let text = match json_str(&params, "text") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.edit_text_by_message_id(&id, &text).await
                });
            }
            "mark_read" => {
                let cid = match json_str(&params, "conversation_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let seq = match json_u64(&params, "read_seq") {
                    Ok(s) => s,
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.mark_read(&cid, seq).await
                });
            }
            "mark_read_with_ids" => {
                let cid = match json_str(&params, "conversation_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let ids = match json_vec_str(&params, "message_ids") {
                    Ok(v) => v,
                    Err(()) => bad_param!(),
                };
                let seq = match json_u64(&params, "read_seq") {
                    Ok(s) => s,
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.mark_read_with_ids(&cid, ids, seq).await
                });
            }
            "typing" => {
                let cid = match json_str(&params, "conversation_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let typing = match json_bool(&params, "typing") {
                    Ok(b) => b,
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.typing(&cid, typing).await
                });
            }
            "add_reaction" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let emoji = match json_str(&params, "emoji") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.add_reaction(&id, &emoji).await
                });
            }
            "remove_reaction" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let emoji = match json_str(&params, "emoji") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.remove_reaction(&id, &emoji).await
                });
            }
            "pin_by_message_id" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.pin_by_message_id(&id).await
                });
            }
            "unpin_by_message_id" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.unpin_by_message_id(&id).await
                });
            }
            "mark_by_message_id" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let mt = match json_i32(&params, "mark_type") {
                    Ok(t) => parse_mark_type(t),
                    Err(()) => bad_param!(),
                };
                let color = match json_str(&params, "color") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.mark_by_message_id(&id, mt, &color).await
                });
            }
            "unmark_by_message_id" => {
                let id = match json_str(&params, "message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let mt = match json_i32(&params, "mark_type") {
                    Ok(t) => parse_mark_type(t),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.message()?;
                    api.unmark_by_message_id(&id, mt).await
                });
            }
            "capability_list" => {
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.capability()?;
                        api.list_capabilities().await
                    },
                    |list| to_json_string(&list),
                );
            }
            "capability_list_user" => {
                let tenant_id = params
                    .get("tenant_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let user_id = params
                    .get("user_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.capability()?;
                        api.list_user_capabilities(tenant_id.as_deref(), user_id.as_deref())
                            .await
                    },
                    |list| to_json_string(&list),
                );
            }
            "capability_dispatch" => {
                let capability_id = match json_str(&params, "capability_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let payload = params
                    .get("payload")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let conversation_id = params
                    .get("conversation_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let tenant_id = params
                    .get("tenant_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let user_id = params
                    .get("user_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let api = client.capability()?;
                        api.dispatch(
                            &capability_id,
                            payload,
                            conversation_id.as_deref(),
                            tenant_id.as_deref(),
                            user_id.as_deref(),
                        )
                        .await
                    },
                    |result| to_json_string(&result),
                );
            }
            "send_call_signal" => {
                let kind = match json_str(&params, "kind") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let conversation_id = match json_str(&params, "conversation_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let call_id = match json_str(&params, "call_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let to_user_id = params
                    .get("to_user_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let video = params
                    .get("video")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                let reason = params
                    .get("reason")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "hangup".to_string());
                let code = params
                    .get("code")
                    .and_then(|x| x.as_i64())
                    .map(|v| v as i32)
                    .unwrap_or(486);
                let close_room_if_vacant =
                    params.get("close_room_if_vacant").and_then(|x| x.as_bool());
                let participant_user_ids: Vec<String> = params
                    .get("participant_user_ids")
                    .or_else(|| params.get("participantUserIds"))
                    .and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                execute_async_unit(instance, ctx, async move {
                    let from = client
                        .current_user_id()
                        .await
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if from.is_empty() {
                        return Err(flare_im_core_sdk::error::FlareError::general_error(
                            "not logged in",
                        ));
                    }
                    let media_types: Vec<CallMediaType> = if video {
                        vec![CallMediaType::Audio, CallMediaType::Video]
                    } else {
                        vec![CallMediaType::Audio]
                    };
                    let mut ev = match kind.as_str() {
                        "invite" => {
                            flare_im_core_sdk::capability::call_event::call_invite_for_conversation(
                                conversation_id.clone(),
                                call_id.clone(),
                                from,
                                to_user_id.clone().unwrap_or_default(),
                                participant_user_ids,
                                media_types.as_slice(),
                            )
                            .map_err(|e| {
                                flare_im_core_sdk::error::FlareError::general_error(&e.to_string())
                            })?
                        }
                        "accept" => flare_im_core_sdk::capability::call_event::call_accept(
                            conversation_id.clone(),
                            call_id.clone(),
                            from,
                            media_types.as_slice(),
                        ),
                        "reject" => flare_im_core_sdk::capability::call_event::call_reject(
                            conversation_id.clone(),
                            call_id.clone(),
                            from,
                            reason,
                            code,
                        ),
                        "hangup" => {
                            flare_im_core_sdk::capability::call_event::call_hangup_with_room_policy(
                                conversation_id.clone(),
                                call_id.clone(),
                                from,
                                reason,
                                None,
                                close_room_if_vacant,
                            )
                        }
                        _ => {
                            return Err(flare_im_core_sdk::error::FlareError::general_error(
                                "invalid call signal kind",
                            ));
                        }
                    };
                    if kind != "invite" {
                        flare_im_core_sdk::call_plugin::apply_session_signaling_audience(
                            &conversation_id,
                            &mut ev,
                            to_user_id.as_deref(),
                        );
                    }
                    client.send_call_signal(&conversation_id, ev).await
                });
            }
            "capability_grant" => {
                let tenant_id = match json_str(&params, "tenant_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let user_id = match json_str(&params, "user_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let capability_id = match json_str(&params, "capability_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let expires_at = params
                    .get("expires_at_rfc3339")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let plan_code = params
                    .get("plan_code")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let source = params
                    .get("source")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async_unit(instance, ctx, async move {
                    let api = client.capability()?;
                    api.grant_user_capability(
                        &tenant_id,
                        &user_id,
                        &capability_id,
                        expires_at.as_deref(),
                        plan_code.as_deref(),
                        source.as_deref(),
                    )
                    .await
                });
            }
            "capability_revoke" => {
                let tenant_id = match json_str(&params, "tenant_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let user_id = match json_str(&params, "user_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let capability_id = match json_str(&params, "capability_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async_unit(instance, ctx, async move {
                    let api = client.capability()?;
                    api.revoke_user_capability(&tenant_id, &user_id, &capability_id)
                        .await
                });
            }
            _ => {
                return_error(&ctx, FLARE_ERR_INVALID_PARAM, "unknown message dispatch op");
                return FLARE_ERR_INVALID_PARAM;
            }
        }
        0
    })
}

/// 请求 JSON：`{ "op": "create_text", "conversation_id": "...", ... }`，与 [`MessageBuildApi`] 的 `create_*` 对齐。
#[unsafe(no_mangle)]
pub extern "C" fn flare_message_build_json(
    handle: FlareHandle,
    request_json: *const c_char,
    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {
    abi::catch_ffi_i32(|| {
        let instance = match require_instance(handle) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let req: serde_json::Value = match parse_json(request_json) {
            Ok(p) => p,
            Err(code) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid request JSON");
                return code;
            }
        };
        let op = match json_str(&req, "op") {
            Ok(s) => s.to_string(),
            Err(()) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, FLARE_ERR_INVALID_PARAM, "missing op");
                return FLARE_ERR_INVALID_PARAM;
            }
        };

        let ctx = CallbackContext::new(context, callback);
        let client = instance.client.clone();

        macro_rules! bad_param {
            () => {{
                return_error(
                    &ctx,
                    FLARE_ERR_INVALID_PARAM,
                    "missing or invalid JSON field",
                );
                return FLARE_ERR_INVALID_PARAM;
            }};
        }

        macro_rules! cid {
            () => {
                match json_str(&req, "conversation_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                }
            };
        }

        match op.as_str() {
            "create_text" => {
                let conversation_id = cid!();
                let text = match json_str(&req, "text") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_text(&conversation_id, &text).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_quote" => {
                let conversation_id = cid!();
                let quoted_message_id = match json_str(&req, "quoted_message_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let text = match json_str(&req, "text") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let quoted_sender_id = req
                    .get("quoted_sender_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let quoted_text_preview = req
                    .get("quoted_text_preview")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let quoted_content = match req.get("quoted_content") {
                    Some(v) => match built_content_from_value(v) {
                        Ok(bc) => Some(bc),
                        Err(code) => {
                            return_error(&ctx, code, "quoted_content");
                            return code;
                        }
                    },
                    None => None,
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_quote(
                            &conversation_id,
                            &quoted_message_id,
                            &text,
                            quoted_sender_id.as_deref(),
                            quoted_text_preview.as_deref(),
                            quoted_content,
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_thread_reply" => {
                let conversation_id = cid!();
                let thread_id = match json_str(&req, "thread_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let text = match json_str(&req, "text") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_thread_reply(&conversation_id, &thread_id, &text)
                            .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_forward" => {
                let conversation_id = cid!();
                let merge = json_bool(&req, "merge").unwrap_or(false);
                let title =
                    match json_str(&req, "forward_title").or_else(|_| json_str(&req, "title")) {
                        Ok(s) => s.to_string(),
                        Err(()) => bad_param!(),
                    };
                let sources = match json_vec_message(&req, "source_messages") {
                    Ok(v) => v,
                    Err(e) => {
                        return_error(&ctx, e, "source_messages");
                        return e;
                    }
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_forward(&conversation_id, merge, &title, sources)
                            .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_image" => {
                let conversation_id = cid!();
                let image_id = match json_str(&req, "image_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_image(&conversation_id, &image_id).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_video" => {
                let conversation_id = cid!();
                let video_id = match json_str(&req, "video_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_video(&conversation_id, &video_id).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_audio" => {
                let conversation_id = cid!();
                let audio_id = match json_str(&req, "audio_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_audio(&conversation_id, &audio_id).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_file" => {
                let conversation_id = cid!();
                let file_id = match json_str(&req, "file_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_file(&conversation_id, &file_id).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_emoji" => {
                let conversation_id = cid!();
                let emoji = match json_str(&req, "emoji") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_emoji(&conversation_id, &emoji).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_location" => {
                let conversation_id = cid!();
                let Some(lon) = req.get("longitude").and_then(|x| x.as_f64()) else {
                    bad_param!()
                };
                let Some(lat) = req.get("latitude").and_then(|x| x.as_f64()) else {
                    bad_param!()
                };
                let address = req
                    .get("address")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = req
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let zoom = req
                    .get("zoom")
                    .and_then(|x| x.as_u64())
                    .map(|z| z.min(255) as u8);
                let snapshot_url = req
                    .get("snapshot_url")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let snapshot_local_path = req
                    .get("snapshot_local_path")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_location(
                            &conversation_id,
                            lon,
                            lat,
                            address,
                            title,
                            zoom,
                            snapshot_url,
                            snapshot_local_path,
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_sticker" => {
                let conversation_id = cid!();
                let sticker_id = match json_str(&req, "sticker_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let package_id = req
                    .get("package_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let url = req
                    .get("url")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let width = req.get("width").and_then(|x| x.as_i64()).map(|i| i as i32);
                let height = req.get("height").and_then(|x| x.as_i64()).map(|i| i as i32);
                let sticker_format = req
                    .get("sticker_format")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_sticker(
                            &conversation_id,
                            &sticker_id,
                            package_id.as_deref(),
                            url.as_deref(),
                            width,
                            height,
                            sticker_format.as_deref(),
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_link_card" => {
                let conversation_id = cid!();
                let url = match json_str(&req, "url") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let title = req
                    .get("title")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let description = req
                    .get("description")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let thumbnail_url = req
                    .get("thumbnail_url")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let site_name = req
                    .get("site_name")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_link_card(
                            &conversation_id,
                            &url,
                            title.as_deref(),
                            description.as_deref(),
                            thumbnail_url.as_deref(),
                            site_name.as_deref(),
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_card" => {
                let conversation_id = cid!();
                let id = match json_str(&req, "id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let card_type = req
                    .get("card_type")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let title = req
                    .get("title")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let subtitle = req
                    .get("subtitle")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let avatar = req
                    .get("avatar")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_card(
                            &conversation_id,
                            &id,
                            card_type.as_deref(),
                            title.as_deref(),
                            subtitle.as_deref(),
                            avatar.as_deref(),
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_mini_program" => {
                let conversation_id = cid!();
                let app_id = match json_str(&req, "app_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let title = req
                    .get("title")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let page_path = req
                    .get("page_path")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let thumbnail_url = req
                    .get("thumbnail_url")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let extra: Option<std::collections::HashMap<String, String>> = req
                    .get("extra")
                    .and_then(|x| serde_json::from_value(x.clone()).ok());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_mini_program(
                            &conversation_id,
                            &app_id,
                            title.as_deref(),
                            page_path.as_deref(),
                            thumbnail_url.as_deref(),
                            extra,
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_rich_doc" => {
                let conversation_id = cid!();
                let doc_json = match json_str(&req, "doc_json") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let content_schema = match json_str(&req, "content_schema") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let plain_text = match json_str(&req, "plain_text") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let input_format = req
                    .get("input_format")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let input_format_version = req
                    .get("input_format_version")
                    .and_then(|x| x.as_i64())
                    .map(|i| i as i32);
                let source_payload: Option<std::collections::HashMap<String, String>> = req
                    .get("source_payload")
                    .and_then(|x| serde_json::from_value(x.clone()).ok());
                let title = req
                    .get("title")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let search_text = req
                    .get("search_text")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let render_hints_json = req
                    .get("render_hints_json")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_rich_doc(
                            &conversation_id,
                            &doc_json,
                            &content_schema,
                            &plain_text,
                            input_format.as_deref(),
                            input_format_version,
                            source_payload,
                            title.as_deref(),
                            search_text.as_deref(),
                            render_hints_json.as_deref(),
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_system" => {
                let conversation_id = cid!();
                let event_kind = match json_str(&req, "event_kind") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let body = match json_str(&req, "body") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_system(&conversation_id, &event_kind, &body).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_notification" => {
                let conversation_id = cid!();
                let title = match json_str(&req, "title") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let body = match json_str(&req, "body") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_notification(&conversation_id, &title, &body).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_vote" => {
                let conversation_id = cid!();
                let vote_id = match json_str(&req, "vote_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let title = match json_str(&req, "title") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let options = match json_vec_str(&req, "options") {
                    Ok(v) => v,
                    Err(()) => bad_param!(),
                };
                let participant_user_ids = req
                    .get("participant_user_ids")
                    .and_then(|x| serde_json::from_value(x.clone()).ok());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_vote(
                            &conversation_id,
                            &vote_id,
                            &title,
                            options,
                            participant_user_ids,
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_task" => {
                let conversation_id = cid!();
                let task_id = match json_str(&req, "task_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let title = match json_str(&req, "title") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let status = req
                    .get("status")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let participant_user_ids = req
                    .get("participant_user_ids")
                    .and_then(|x| serde_json::from_value(x.clone()).ok());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_task(
                            &conversation_id,
                            &task_id,
                            &title,
                            status.as_deref(),
                            participant_user_ids,
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_schedule" => {
                let conversation_id = cid!();
                let schedule_id = match json_str(&req, "schedule_id") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let title = match json_str(&req, "title") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let start_ms = match req.get("start_time_ms").and_then(|x| x.as_i64()) {
                    Some(v) => v,
                    None => bad_param!(),
                };
                let end_ms = match req.get("end_time_ms").and_then(|x| x.as_i64()) {
                    Some(v) => v,
                    None => bad_param!(),
                };
                let participant_user_ids = req
                    .get("participant_user_ids")
                    .and_then(|x| serde_json::from_value(x.clone()).ok());
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_schedule(
                            &conversation_id,
                            &schedule_id,
                            &title,
                            start_ms,
                            end_ms,
                            participant_user_ids,
                        )
                        .await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_announcement" => {
                let conversation_id = cid!();
                let title = match json_str(&req, "title") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                let body = match json_str(&req, "body") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_announcement(&conversation_id, &title, &body).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_custom" => {
                let conversation_id = cid!();
                let t = match json_str(&req, "type") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_custom(&conversation_id, &t).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_placeholder" => {
                let conversation_id = cid!();
                let reason = match json_str(&req, "reason") {
                    Ok(s) => s.to_string(),
                    Err(()) => bad_param!(),
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_placeholder(&conversation_id, &reason).await
                    },
                    |m| to_json_string(&m),
                );
            }
            "create_with_content" => {
                let conversation_id = cid!();
                let content = match req.get("content") {
                    Some(v) => match built_content_from_value(v) {
                        Ok(c) => c,
                        Err(code) => {
                            return_error(&ctx, code, "content");
                            return code;
                        }
                    },
                    None => {
                        return_error(&ctx, FLARE_ERR_INVALID_PARAM, "missing content");
                        return FLARE_ERR_INVALID_PARAM;
                    }
                };
                execute_async(
                    instance,
                    ctx,
                    async move {
                        let b = client.message_build()?;
                        b.create_with_content(&conversation_id, content).await
                    },
                    |m| to_json_string(&m),
                );
            }
            _ => {
                return_error(&ctx, FLARE_ERR_INVALID_PARAM, "unknown message_build op");
                return FLARE_ERR_INVALID_PARAM;
            }
        }
        0
    })
}
