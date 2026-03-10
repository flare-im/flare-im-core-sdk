//! 会话命令：列表、详情、已读、删除、创建会话 ID、输入状态等

use tauri::State;

use crate::convert::conversation_to_model;
use crate::model::ConversationSummaryOut;
use crate::state::SdkState;

/// 获取会话列表
#[tauri::command]
pub async fn sdk_get_conversations(state: State<'_, SdkState>) -> std::result::Result<Vec<ConversationSummaryOut>, String> {
    let list = state
        .with_client(|c| {
            Box::pin(async move {
                c.conversation().list().await.map_err(|e| e.to_string())
            })
        })
        .await?;
    let out: Vec<ConversationSummaryOut> = list.iter().map(conversation_to_model).collect();
    Ok(out)
}

/// 获取单个会话
#[tauri::command]
pub async fn sdk_get_one_conversation(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<Option<ConversationSummaryOut>, String> {
    let opt = state
        .with_client(|c| {
            Box::pin(async move {
                c.conversation().get(&conversation_id).await.map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(opt.map(|c| conversation_to_model(&c)))
}

/// 获取会话 ID（单聊/群聊；单聊时当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_get_conversation_id_by_session_type(
    state: State<'_, SdkState>,
    session_type: String,
    peer_id: Option<String>,
    group_id: Option<String>,
) -> std::result::Result<String, String> {
    let id = state
        .with_client::<String, _, String>(|c| {
            Box::pin(async move {
                let conv = c.conversation();
                let id = match session_type.as_str() {
                    "single" | "Single" | "private" => {
                        let peer = peer_id.as_deref().ok_or("peer_id required for single chat")?;
                        conv.single_chat_id_for_current_user(peer).await.map_err(|e| e.to_string())?
                    }
                    "group" | "Group" => {
                        let gid = group_id.as_deref().ok_or("group_id required for group")?;
                        conv.group_id(gid)
                    }
                    _ => return Err("unsupported session_type".into()),
                };
                Ok(id)
            })
        })
        .await?;
    Ok(id)
}

/// 总未读数
#[tauri::command]
pub async fn sdk_get_total_unread_msg_count(state: State<'_, SdkState>) -> std::result::Result<u32, String> {
    let list = state
        .with_client(|c| {
            Box::pin(async move { c.conversation().list().await.map_err(|e| e.to_string()) })
        })
        .await?;
    Ok(list.iter().map(|c| c.unread_count).sum())
}

/// 标记会话已读
#[tauri::command]
pub async fn sdk_mark_conversation_as_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    read_seq: Option<u64>,
) -> std::result::Result<(), String> {
    let seq = read_seq.unwrap_or(u64::MAX);
    state
        .with_client(|c| {
            Box::pin(async move {
            c.conversation()
                .mark_read(&conversation_id, seq)
                .await
                .map_err(|e| e.to_string())
        })
        })
        .await?;
    Ok(())
}

/// 全部已读（SDK 内拉列表并逐条 mark_read，统一逻辑）
#[tauri::command]
pub async fn sdk_mark_all_read(state: State<'_, SdkState>) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
                c.conversation()
                    .mark_all_read()
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 删除会话（本地）
#[tauri::command]
pub async fn sdk_delete_conversation(
    state: State<'_, SdkState>,
    conversation_id: String,
) -> std::result::Result<(), String> {
    state
        .with_client(|c| {
            Box::pin(async move {
            c.conversation()
                .delete(&conversation_id)
                .await
                .map_err(|e| e.to_string())
        })
        })
        .await?;
    Ok(())
}

/// 创建会话：生成单聊/群聊 ID、触发同步，并在本地列表中写入占位会话以便立即可见（当前用户从 SDK 获取）。
#[tauri::command]
pub async fn sdk_create_session(
    state: State<'_, SdkState>,
    session_type: String,
    business_type: String,
    display_name: Option<String>,
    peer_id: Option<String>,
) -> std::result::Result<String, String> {
    let conv_id = state
        .with_client::<String, _, String>(|c| {
            Box::pin(async move {
                let conv = c.conversation();
                let id = match session_type.as_str() {
                    "single" | "Single" | "private" => {
                        let peer = peer_id.as_deref().ok_or("peer_id required for single chat")?;
                        conv.single_chat_id_for_current_user(peer).await.map_err(|e| e.to_string())?
                    }
                    "group" | "Group" => {
                        let gid = peer_id.as_deref().unwrap_or("default-group");
                        conv.group_id(gid)
                    }
                    _ => return Err("unsupported session_type".into()),
                };
                c.sync_conversation(&id).await.map_err(|e| e.to_string())?;
                conv.ensure_local_conversation(
                    &id,
                    display_name.as_deref(),
                    &session_type,
                    &business_type,
                    peer_id.as_deref(),
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(id)
            })
        })
        .await?;
    Ok(conv_id)
}

/// 正在输入状态（映射到 message().typing；当前用户从 SDK 获取）
#[tauri::command]
pub async fn sdk_set_input_state(
    state: State<'_, SdkState>,
    conversation_id: String,
    state_type: String,
) -> std::result::Result<(), String> {
    let typing = state_type.eq_ignore_ascii_case("Typing");
    state
        .with_client(|c| {
            Box::pin(async move {
                c.message()
                    .typing(&conversation_id, typing)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .await?;
    Ok(())
}

/// 分页会话列表（与 list 一致，前端兼容）
#[tauri::command]
pub async fn sdk_get_conversation_list_split(
    state: State<'_, SdkState>,
    _cursor: Option<String>,
    _limit: Option<u32>,
) -> std::result::Result<Vec<ConversationSummaryOut>, String> {
    sdk_get_conversations(state).await
}

/// 批量获取会话（前端兼容）
#[tauri::command]
pub async fn sdk_get_multiple_conversation(
    state: State<'_, SdkState>,
    conversation_ids: Vec<String>,
) -> std::result::Result<Vec<ConversationSummaryOut>, String> {
    let ids = conversation_ids;
    let out = state
        .with_client::<Vec<ConversationSummaryOut>, _, String>(|c| {
            Box::pin(async move {
                let mut list = Vec::with_capacity(ids.len());
                for id in &ids {
                    if let Ok(Some(summary)) = c.conversation().get(id).await {
                        list.push(conversation_to_model(&summary));
                    }
                }
                Ok(list)
            })
        })
        .await?;
    Ok(out)
}

/// 获取输入状态（占位，SDK 无直接接口）
#[tauri::command]
pub async fn sdk_get_input_states(_state: State<'_, SdkState>, _conversation_id: String) -> std::result::Result<Vec<crate::model::InputStateOut>, String> {
    Ok(Vec::new())
}

/// 设置草稿（占位，存本地或扩展）
#[tauri::command]
pub async fn sdk_set_conversation_draft(
    _state: State<'_, SdkState>,
    _conversation_id: String,
    _draft: Option<String>,
) -> std::result::Result<(), String> {
    Ok(())
}

/// 隐藏会话（占位）
#[tauri::command]
pub async fn sdk_hide_conversation(
    _state: State<'_, SdkState>,
    _conversation_id: String,
) -> std::result::Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn sdk_hide_all_conversations(_state: State<'_, SdkState>) -> std::result::Result<(), String> {
    Ok(())
}

/// 清空会话消息（占位，或本地删除）
#[tauri::command]
pub async fn sdk_clear_conversation_messages(
    _state: State<'_, SdkState>,
    _conversation_id: String,
) -> std::result::Result<(), String> {
    Ok(())
}

/// 设置会话信息（占位）
#[tauri::command]
pub async fn sdk_set_conversation_info(
    _state: State<'_, SdkState>,
    _conversation_id: String,
    _display_name: Option<String>,
    _avatar_url: Option<String>,
) -> std::result::Result<(), String> {
    Ok(())
}
