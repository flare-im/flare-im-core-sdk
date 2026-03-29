//! 消息命令：透传 [MessageBuildApi] / [MessageApi]。

use tauri::State;

use crate::model::SendAckPayload;
use crate::state::SdkState;
use flare_im_core_sdk::model::IMMessage;
use flare_im_core_sdk::model::message::MarkType;

// ========== MessageBuildApi：create_* 返回 IMMessage，由前端再调 send 发送 ==========

#[tauri::command]
pub async fn sdk_create_text(
    state: State<'_, SdkState>,
    conversation_id: String,
    text: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_text(&conversation_id, &text)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_quote(
    state: State<'_, SdkState>,
    conversation_id: String,
    quoted_message_id: String,
    text: String,
    quoted_text_preview: Option<String>,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_quote(
                    &conversation_id,
                    &quoted_message_id,
                    &text,
                    quoted_text_preview.as_deref(),
                )
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_thread_reply(
    state: State<'_, SdkState>,
    conversation_id: String,
    thread_id: String,
    text: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_thread_reply(&conversation_id, &thread_id, &text)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_forward(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_ids: Vec<String>,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_forward(&conversation_id, message_ids)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_image(
    state: State<'_, SdkState>,
    conversation_id: String,
    image_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_image(&conversation_id, &image_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_video(
    state: State<'_, SdkState>,
    conversation_id: String,
    video_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_video(&conversation_id, &video_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_audio(
    state: State<'_, SdkState>,
    conversation_id: String,
    audio_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_audio(&conversation_id, &audio_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_file(
    state: State<'_, SdkState>,
    conversation_id: String,
    file_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_file(&conversation_id, &file_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_location(
    state: State<'_, SdkState>,
    conversation_id: String,
    longitude: f64,
    latitude: f64,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_location(&conversation_id, longitude, latitude)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_card(
    state: State<'_, SdkState>,
    conversation_id: String,
    user_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_card(&conversation_id, &user_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_sticker(
    state: State<'_, SdkState>,
    conversation_id: String,
    sticker_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_sticker(&conversation_id, &sticker_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_emoji(
    state: State<'_, SdkState>,
    conversation_id: String,
    emoji: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_emoji(&conversation_id, &emoji)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_gif(
    state: State<'_, SdkState>,
    conversation_id: String,
    gif_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_gif(&conversation_id, &gif_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_link_card(
    state: State<'_, SdkState>,
    conversation_id: String,
    url: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_link_card(&conversation_id, &url)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_mini_program(
    state: State<'_, SdkState>,
    conversation_id: String,
    app_id: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_mini_program(&conversation_id, &app_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_rich_text(
    state: State<'_, SdkState>,
    conversation_id: String,
    body: String,
    format: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_rich_text(&conversation_id, &body, &format)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_markdown(
    state: State<'_, SdkState>,
    conversation_id: String,
    text: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_markdown(&conversation_id, &text)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_system(
    state: State<'_, SdkState>,
    conversation_id: String,
    event_kind: String,
    body: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_system(&conversation_id, &event_kind, &body)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_notification(
    state: State<'_, SdkState>,
    conversation_id: String,
    title: String,
    body: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_notification(&conversation_id, &title, &body)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_vote(
    state: State<'_, SdkState>,
    conversation_id: String,
    vote_id: String,
    title: String,
    options: Vec<String>,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_vote(&conversation_id, &vote_id, &title, options)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_task(
    state: State<'_, SdkState>,
    conversation_id: String,
    task_id: String,
    title: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_task(&conversation_id, &task_id, &title)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_schedule(
    state: State<'_, SdkState>,
    conversation_id: String,
    schedule_id: String,
    title: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_schedule(&conversation_id, &schedule_id, &title)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_announcement(
    state: State<'_, SdkState>,
    conversation_id: String,
    title: String,
    body: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_announcement(&conversation_id, &title, &body)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_custom(
    state: State<'_, SdkState>,
    conversation_id: String,
    r#type: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_custom(&conversation_id, &r#type)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_create_placeholder(
    state: State<'_, SdkState>,
    conversation_id: String,
    reason: String,
) -> std::result::Result<IMMessage, String> {
    let c = state.client();
    c.message_build().map_err(|e| e.to_string())?
        .create_placeholder(&conversation_id, &reason)
        .await
        .map_err(|e| e.to_string())

}

// ========== MessageApi：发送与读写 ==========

#[tauri::command]
pub async fn sdk_send(
    state: State<'_, SdkState>,
    message: IMMessage,
) -> std::result::Result<SendAckPayload, String> {
    let c = state.client();
    let ack = c
        .message()
        .map_err(|e| e.to_string())?
        .send(message)
        .await
        .map_err(|e| e.to_string())?;
    Ok(ack.into())
}

#[tauri::command]
pub async fn sdk_recall(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .recall(&message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_edit(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_id: String,
    new_content: Vec<u8>,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .edit(&conversation_id, &message_id, new_content)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_edit_text_by_message_id(
    state: State<'_, SdkState>,
    message_id: String,
    text: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .edit_text_by_message_id(&message_id, &text)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_delete_message(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .delete(&message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_mark_read(
    state: State<'_, SdkState>,
    conversation_id: String,
    read_seq: u64,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .mark_read(&conversation_id, read_seq)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_mark_read_with_ids(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_ids: Vec<String>,
    read_seq: u64,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .mark_read_with_ids(&conversation_id, message_ids, read_seq)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_typing(
    state: State<'_, SdkState>,
    conversation_id: String,
    typing: bool,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .typing(&conversation_id, typing)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_add_reaction(
    state: State<'_, SdkState>,
    message_id: String,
    emoji: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .add_reaction(&message_id, &emoji)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_remove_reaction(
    state: State<'_, SdkState>,
    message_id: String,
    emoji: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .remove_reaction(&message_id, &emoji)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_pin(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .pin(&conversation_id, &message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_unpin(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .unpin(&conversation_id, &message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_pin_by_message_id(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .pin_by_message_id(&message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_unpin_by_message_id(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<(), String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .unpin_by_message_id(&message_id)
        .await
        .map_err(|e| e.to_string())

}

fn parse_mark_type(v: i32) -> MarkType {
    match v {
        1 => MarkType::Important,
        2 => MarkType::Todo,
        3 => MarkType::Done,
        _ => MarkType::Custom,
    }
}

#[tauri::command]
pub async fn sdk_mark(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_id: String,
    mark_type: i32,
) -> std::result::Result<(), String> {
    let mt = parse_mark_type(mark_type);
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .mark(&conversation_id, &message_id, mt)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_mark_with_color(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_id: String,
    mark_type: i32,
    color: String,
) -> std::result::Result<(), String> {
    let mt = parse_mark_type(mark_type);
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .mark_with_color(&conversation_id, &message_id, mt, &color)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_unmark(
    state: State<'_, SdkState>,
    conversation_id: String,
    message_id: String,
    mark_type: i32,
) -> std::result::Result<(), String> {
    let mt = parse_mark_type(mark_type);
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .unmark(&conversation_id, &message_id, mt)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_mark_by_message_id(
    state: State<'_, SdkState>,
    message_id: String,
    mark_type: i32,
    color: String,
) -> std::result::Result<(), String> {
    let mt = parse_mark_type(mark_type);
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .mark_by_message_id(&message_id, mt, &color)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_unmark_by_message_id(
    state: State<'_, SdkState>,
    message_id: String,
    mark_type: i32,
) -> std::result::Result<(), String> {
    let mt = parse_mark_type(mark_type);
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .unmark_by_message_id(&message_id, mt)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_get_message(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<Option<IMMessage>, String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .get(&message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_get_message_raw(
    state: State<'_, SdkState>,
    message_id: String,
) -> std::result::Result<Option<IMMessage>, String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .get_raw(&message_id)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_list_messages(
    state: State<'_, SdkState>,
    conversation_id: String,
    before_seq: u64,
    limit: u32,
) -> std::result::Result<Vec<IMMessage>, String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .list(&conversation_id, before_seq, limit)
        .await
        .map_err(|e| e.to_string())

}

#[tauri::command]
pub async fn sdk_search_messages(
    state: State<'_, SdkState>,
    keyword: String,
    limit: u32,
) -> std::result::Result<Vec<IMMessage>, String> {
    let c = state.client();
    c.message().map_err(|e| e.to_string())?
        .search(&keyword, limit)
        .await
        .map_err(|e| e.to_string())

}
