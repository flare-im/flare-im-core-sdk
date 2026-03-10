//! 消息命令：仅调用 SDK 高层 API，业务逻辑均在 SDK 内

use tauri::State;

use crate::convert::message_to_model;
use crate::state::SdkState;

/// 发送纯文本消息（单聊可传 receiver_id，群聊不传）。
/// 若未传 receiver_id 且会话为单聊（conversation_id 以 1A 开头），则从本地会话的 ext.peer_id 解析并填充，避免服务端报 code=60。
#[tauri::command]
pub async fn sdk_send_text_message(
    state: State<'_, SdkState>,
    session_id: String,
    text: String,
    receiver_id: Option<String>,
) -> std::result::Result<String, String> {
    let ack = state
        .with_client(|c| {
            Box::pin(async move {
                let resolved: Option<String> = match &receiver_id {
                    Some(r) => Some(r.clone()),
                    None if flare_im_core_sdk::conversation::is_single_chat_conversation(&session_id) => {
                        let conv = c.conversation().get(&session_id).await.map_err(|e| e.to_string())?;
                        match conv.and_then(|c| c.ext.get("peer_id").cloned()) {
                            Some(peer) => Some(peer),
                            None => {
                                return Err("单聊消息需要 receiver_id，且本地会话缺少 peer_id。请通过「新会话」创建会话时填写对方用户 ID。".to_string());
                            }
                        }
                    }
                    _ => None,
                };
                c.message()
                    .send_text_message(&session_id, &text, resolved.as_deref())
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(ack.server_msg_id)
}

/// 获取会话消息列表（cursor 可选，格式 seq:before_seq 或 null 表示最新）
#[tauri::command]
pub async fn sdk_get_messages(
    state: State<'_, SdkState>,
    session_id: String,
    limit: u32,
    cursor: Option<String>,
) -> std::result::Result<Vec<crate::model::MessageOut>, String> {
    let before_seq = cursor
        .as_deref()
        .and_then(|c| c.strip_prefix("seq:"))
        .and_then(|s| s.split(':').next())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    let limit = limit.min(100).max(1);

    let list = state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .list(&session_id, before_seq, limit)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;

    let out: Vec<crate::model::MessageOut> = list.iter().map(message_to_model).collect();
    Ok(out)
}

/// 标记会话已读（SDK 内统一处理消息已读回执 + 会话未读；当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_mark_session_read(
    state: State<'_, SdkState>,
    session_id: String,
    last_seq: Option<u64>,
) -> std::result::Result<(), String> {
    let read_seq = last_seq.unwrap_or(u64::MAX);
    state
        .with_client(|c| {
            Box::pin(async move {
                c.mark_session_read(&session_id, read_seq)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 撤回消息
#[tauri::command]
pub async fn sdk_recall_message(
    state: State<'_, SdkState>,
    message_id: String,
    _reason: Option<String>,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .recall_by_message_id(&message_id)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 编辑消息（文本）
#[tauri::command]
pub async fn sdk_edit_message(
    state: State<'_, SdkState>,
    message_id: String,
    text: String,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .edit_text_by_message_id(&message_id, &text)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 删除消息
#[tauri::command]
pub async fn sdk_delete_message(
    state: State<'_, SdkState>,
    message_id: String,
    _delete_type: Option<i32>,
    _reason: Option<String>,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .delete_by_message_id(&message_id)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 添加表情反应
#[tauri::command]
pub async fn sdk_add_reaction(
    state: State<'_, SdkState>,
    message_id: String,
    emoji: String,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .add_reaction_by_message_id(&message_id, &emoji)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 移除表情反应（当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_remove_reaction(
    state: State<'_, SdkState>,
    message_id: String,
    emoji: String,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .remove_reaction_by_message_id(&message_id, &emoji)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 置顶消息
#[tauri::command]
pub async fn sdk_pin_message(
    state: State<'_, SdkState>,
    message_id: String,
    _reason: Option<String>,
    _expire_at: Option<String>,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .pin_by_message_id(&message_id)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 取消置顶
#[tauri::command]
pub async fn sdk_unpin_message(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .unpin_by_message_id(&message_id)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 标记消息（重要/待办等；当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_mark_message(
    state: State<'_, SdkState>,
    message_id: String,
    mark_type: i32,
    color: Option<String>,
) -> std::result::Result<(), String> {
    use flare_im_core_sdk::model::message::MarkType;
    let mt = match mark_type {
        1 => MarkType::Important,
        2 => MarkType::Todo,
        3 => MarkType::Done,
        _ => MarkType::Custom,
    };
    let color_owned = color.unwrap_or_default();
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .mark_by_message_id(&message_id, mt, &color_owned)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 引用消息发送（当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_quote_message(
    state: State<'_, SdkState>,
    session_id: String,
    quoted_message_id: String,
    text: String,
    preview_text: Option<String>,
) -> std::result::Result<String, String> {
    let ack = state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .send_quote_message(
                        &session_id,
                        &quoted_message_id,
                        &text,
                        preview_text.as_deref(),
                    )
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(ack.server_msg_id)
}

/// 回复消息（内部调用引用发送）
#[tauri::command]
pub async fn sdk_reply_message(
    state: State<'_, SdkState>,
    session_id: String,
    reply_to_message_id: String,
    text: String,
) -> std::result::Result<String, String> {
    sdk_quote_message(state, session_id, reply_to_message_id, text, None).await
}

/// 线程回复（当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_add_thread_reply(
    state: State<'_, SdkState>,
    session_id: String,
    thread_id: String,
    text: String,
) -> std::result::Result<String, String> {
    let ack = state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .send_thread_reply(&session_id, &thread_id, &text)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(ack.server_msg_id)
}

/// 转发消息（单条；当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_forward_message(
    state: State<'_, SdkState>,
    message_ids: Vec<String>,
    target_session_id: String,
    _merge_forward: bool,
    _reason: Option<String>,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .forward_message(&target_session_id, message_ids)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 标记已读（当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_mark_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    read_seq: u64,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .mark_read(&conversation_id, read_seq)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 收藏（SDK 内用 MarkType::Important）
#[tauri::command]
pub async fn sdk_favorite_message(
    state: State<'_, SdkState>,
    message_id: String,
    _tags: Option<Vec<String>>,
    _note: Option<String>,
) -> std::result::Result<(), String> {
    sdk_mark_message(state, message_id, 1, None).await
}

/// 取消收藏（当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_unfavorite_message(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<(), String> {
    use flare_im_core_sdk::model::message::MarkType;
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .unmark_by_message_id(&message_id, MarkType::Important)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}
