//! SQLite 消息仓储：与 [schema] 中 messages 表结构一致，按列读写；row 直接映射为 IMMessage（不经 ProtoMessage）。

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use base64::prelude::*;
use flare_proto::common::{MessageRetentionPolicy, MessageRetentionState, ReactionAction};
use prost::Message as ProstMessage;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool, Transaction};
use tracing::debug;

use crate::content::message_elem::{
    AudioInfoElem, ImageInfoElem, TextElem, VideoInfoElem, elem_preview_storage_payload,
};
use crate::domain::{
    EditApplyResult, MessageDeliveryService, MessageReader, MessageStore, MessageWriter,
    OperationApplyResult, local_cleared_through_seq, merge_message_event_attributes,
    message_visible_after_clear,
};
use crate::model::conversation::ConversationType;
use crate::model::message::{
    MessageLocalState, ReactionEntry, has_reaction_snapshot_in_attributes,
    parse_reactions_from_attributes,
};
use crate::model::search::{SqliteKeywordSearch, sqlite_keyword_search};
use crate::model::{
    Elem, IMMessage, MessageSearchKind, MessageSearchQuery, decode_content_bytes,
    decoded_content_to_elem,
};
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::time::now_millis;
use flare_proto::common::{ContentVisibility, MessageStatus, MessageType};

use super::identity_repair;

const MESSAGE_SAVE_BATCH_INSERT_CHUNK_SIZE: usize = 20;
const MESSAGE_REACTION_INSERT_CHUNK_SIZE: usize = 100;

struct MessagePersistRow<'a> {
    message: &'a IMMessage,
    attributes_json: String,
    mention_users_json: String,
    extensions_json: String,
    text: Option<String>,
    search_text: Option<String>,
}

impl<'a> MessagePersistRow<'a> {
    fn from_message(message: &'a IMMessage) -> Self {
        Self {
            message,
            attributes_json: serde_json::to_string(&message.attributes).unwrap_or_default(),
            mention_users_json: serde_json::to_string(&message.mention_users).unwrap_or_default(),
            extensions_json: extensions_to_json(&message.extensions),
            text: message_preview_for_storage(message),
            search_text: message_search_text_for_storage(message),
        }
    }
}

fn parse_extra(s: Option<&str>) -> HashMap<String, String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

fn extra_to_json(attributes: &HashMap<String, String>) -> String {
    serde_json::to_string(attributes).unwrap_or_default()
}

fn encode_optional_proto<M: ProstMessage>(message: &Option<M>) -> Vec<u8> {
    message
        .as_ref()
        .map(ProstMessage::encode_to_vec)
        .unwrap_or_default()
}

fn decode_optional_proto<M>(bytes: Vec<u8>) -> Option<M>
where
    M: ProstMessage + Default,
{
    if bytes.is_empty() {
        return None;
    }
    M::decode(bytes.as_slice()).ok()
}

fn retention_hides_content(state: &MessageRetentionState) -> bool {
    matches!(
        ContentVisibility::try_from(state.content_visibility).ok(),
        Some(ContentVisibility::Hidden | ContentVisibility::Redacted | ContentVisibility::Purged)
    )
}

fn parse_mention_users(s: Option<&str>) -> Vec<String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return Vec::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

fn sqlx_err(e: sqlx::Error) -> FlareError {
    FlareError::localized(ErrorCode::DatabaseError, e.to_string())
}

/// 将分页游标 `before_seq` 绑定为 SQLite INTEGER（`conversation_seq < ?`）。
///
/// - **`0`**：表示客户端刚打开会话、尚无游标，等价于「上界无穷」，取当前库中 **最新一页**（与 Tauri `INITIAL_BEFORE_SEQ` 语义对齐，客户端可直接传 `0`）。
///   最新一页在仓储层按 **本地 seq=0 待发送优先，其余服务端消息按 `conversation_seq DESC`**（见 `get_by_conversation`），**不**伪造服务端 `conversation_seq`。
/// - **`u64::MAX`**：不可直接 `as i64`（会变成 `-1`，导致 `conversation_seq < -1` 恒空），钳制到 `i64::MAX`。
/// - 其它正值：`conversation_seq < before_seq`，用于「加载更早消息」。
fn before_seq_for_sqlite(before_seq: u64) -> i64 {
    if before_seq == 0 || before_seq >= i64::MAX as u64 {
        i64::MAX
    } else {
        before_seq as i64
    }
}

fn u64_to_i64_saturating(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

/// 写入 `messages.sort_ts` 的最终值：仅用于**本地列表**「最新一页」排序，**不**参与多端 `conversation_seq` 同步语义。
///
/// 取 `max(IMMessage::timeline_sort_ts, 墙钟)`，避免仅保留较小入队时间而弱于历史消息、被 `LIMIT` 裁掉。
fn effective_sort_ts_for_persist(message: &IMMessage) -> i64 {
    let wall = now_ms_i64().max(0) as u64;
    let merged = message.timeline_sort_ts().max(wall);
    u64_to_i64_saturating(merged)
}

fn message_preview_for_storage(message: &IMMessage) -> Option<String> {
    message
        .text_for_storage()
        .or_else(|| preview_for_content_bytes(&message.encoded_content))
        .or_else(|| {
            message
                .attributes
                .get("contentText")
                .map(|s| s.trim())
                .filter(|s| !crate::content::preview_storage::is_redundant_content_text_extra(s))
                .map(str::to_string)
        })
}

fn push_search_part(out: &mut String, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(value);
}

fn push_image_info_search_text(out: &mut String, info: &ImageInfoElem) {
    push_search_part(out, &info.uuid);
    push_search_part(out, &info.mime_type);
}

fn push_video_info_search_text(out: &mut String, info: &VideoInfoElem) {
    push_search_part(out, &info.uuid);
    push_search_part(out, &info.mime_type);
}

fn push_audio_info_search_text(out: &mut String, info: &AudioInfoElem) {
    push_search_part(out, &info.uuid);
    push_search_part(out, &info.mime_type);
}

fn elem_search_text(elem: &Elem) -> Option<String> {
    let mut out = String::new();
    match elem {
        Elem::Text(text) => {
            push_search_part(&mut out, &text.text);
        }
        Elem::RichText(rich) => {
            if let Some(title) = &rich.title {
                push_search_part(&mut out, title);
            }
            push_search_part(&mut out, &rich.plain_text);
            if let Some(search_text) = &rich.search_text {
                push_search_part(&mut out, search_text);
            }
        }
        Elem::Image(image) => {
            push_search_part(&mut out, &image.description);
            if let Some(source) = &image.source {
                push_image_info_search_text(&mut out, source);
            }
            if let Some(thumbnail) = &image.thumbnail {
                push_image_info_search_text(&mut out, thumbnail);
            }
        }
        Elem::Video(video) => {
            push_search_part(&mut out, &video.description);
            push_search_part(&mut out, &video.video_id);
            if let Some(source) = &video.source {
                push_video_info_search_text(&mut out, source);
            }
            if let Some(cover) = &video.cover {
                push_image_info_search_text(&mut out, cover);
            }
        }
        Elem::Audio(audio) => {
            push_search_part(&mut out, &audio.description);
            push_search_part(&mut out, &audio.audio_id);
            if let Some(source) = &audio.source {
                push_audio_info_search_text(&mut out, source);
            }
        }
        Elem::File(file) => {
            push_search_part(&mut out, &file.file_name);
            push_search_part(&mut out, &file.description);
            push_search_part(&mut out, &file.mime_type);
            push_search_part(&mut out, &file.file_id);
        }
        Elem::Location(location) => {
            push_search_part(&mut out, &location.title);
            push_search_part(&mut out, &location.address);
        }
        Elem::Card(card) => {
            push_search_part(&mut out, &card.title);
            push_search_part(&mut out, &card.subtitle);
        }
        Elem::Emoji(emoji) => {
            push_search_part(&mut out, &emoji.emoji);
            push_search_part(&mut out, &emoji.description);
        }
        Elem::Quote(quote) => {
            push_search_part(&mut out, &quote.quoted_text_preview);
            if let Some(current) = &quote.current_content
                && let Some(text) = elem_search_text(current)
            {
                push_search_part(&mut out, &text);
            }
            if let Some(quoted) = &quote.quoted_content
                && let Some(text) = elem_search_text(quoted)
            {
                push_search_part(&mut out, &text);
            }
        }
        Elem::LinkCard(link) => {
            push_search_part(&mut out, &link.title);
            push_search_part(&mut out, &link.description);
            push_search_part(&mut out, &link.site_name);
            push_search_part(&mut out, &link.url);
        }
        Elem::Forward(forward) => {
            if let Some(title) = &forward.title {
                push_search_part(&mut out, title);
            }
            for item in &forward.items {
                push_search_part(&mut out, &item.plain_text);
                if let Some(content) = &item.content
                    && let Some(text) = elem_search_text(content)
                {
                    push_search_part(&mut out, &text);
                }
            }
        }
        Elem::Thread(thread) => {
            push_search_part(&mut out, &thread.thread_title);
            if let Some(root) = &thread.root_content
                && let Some(text) = elem_search_text(root)
            {
                push_search_part(&mut out, &text);
            }
        }
        Elem::MiniProgram(mini) => {
            push_search_part(&mut out, &mini.title);
            push_search_part(&mut out, &mini.app_id);
            push_search_part(&mut out, &mini.page_path);
        }
        Elem::ImageGroup(group) => {
            push_search_part(&mut out, &group.description);
            for image in &group.images {
                push_image_info_search_text(&mut out, image);
            }
        }
        Elem::System(system) => {
            push_search_part(&mut out, &system.body);
            push_search_part(&mut out, &system.event_kind);
        }
        Elem::Notification(notification) => {
            push_search_part(&mut out, &notification.title);
            push_search_part(&mut out, &notification.body);
            push_search_part(&mut out, &notification.notification_type);
        }
        Elem::Vote(vote) => {
            push_search_part(&mut out, &vote.title);
            for option in &vote.options {
                push_search_part(&mut out, option);
            }
        }
        Elem::Task(task) => {
            push_search_part(&mut out, &task.title);
            push_search_part(&mut out, &task.status);
        }
        Elem::Schedule(schedule) => {
            push_search_part(&mut out, &schedule.title);
        }
        Elem::Announcement(announcement) => {
            push_search_part(&mut out, &announcement.title);
            push_search_part(&mut out, &announcement.body);
        }
        Elem::Custom(custom) => {
            push_search_part(&mut out, &custom.r#type);
            push_search_part(&mut out, &custom.description);
        }
        Elem::Placeholder(placeholder) => {
            push_search_part(&mut out, &placeholder.fallback_text);
            push_search_part(&mut out, &placeholder.reason);
        }
        Elem::Sticker(sticker) => {
            push_search_part(&mut out, &sticker.sticker_id);
            push_search_part(&mut out, &sticker.package_id);
            push_search_part(&mut out, &sticker.format);
        }
    }
    (!out.is_empty()).then_some(out)
}

fn search_text_for_content_bytes(bytes: &[u8]) -> Option<String> {
    decode_content_bytes(bytes)
        .ok()
        .and_then(|decoded| decoded_content_to_elem(&decoded))
        .and_then(|elem| elem_search_text(&elem))
}

fn message_search_text_for_storage(message: &IMMessage) -> Option<String> {
    if let Some(content) = &message.content
        && let Some(text) = elem_search_text(content)
    {
        return Some(text);
    }
    if let Some(text) = search_text_for_content_bytes(&message.encoded_content) {
        return Some(text);
    }
    message_preview_for_storage(message)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

async fn delete_message_fts_by_server_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
    server_id: &str,
) -> Result<()> {
    sqlx::query("DELETE FROM messages_fts WHERE server_id = ?")
        .bind(server_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

async fn upsert_message_fts_tx(
    tx: &mut Transaction<'_, Sqlite>,
    server_id: &str,
    conversation_id: &str,
    text: Option<&str>,
) -> Result<()> {
    delete_message_fts_by_server_id_tx(tx, server_id).await?;
    let Some(text) = text.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    sqlx::query("INSERT INTO messages_fts(server_id, conversation_id, text) VALUES (?, ?, ?)")
        .bind(server_id)
        .bind(conversation_id)
        .bind(text)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

async fn upsert_message_fts_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[MessagePersistRow<'_>],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut delete_qb =
        QueryBuilder::<Sqlite>::new("DELETE FROM messages_fts WHERE server_id IN (");
    {
        let mut separated = delete_qb.separated(", ");
        for row in rows {
            separated.push_bind(&row.message.server_id);
        }
    }
    delete_qb.push(")");
    delete_qb
        .build()
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;

    let searchable = rows
        .iter()
        .filter_map(|row| {
            row.search_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| (row, text))
        })
        .collect::<Vec<_>>();
    if searchable.is_empty() {
        return Ok(());
    }

    let mut insert_qb =
        QueryBuilder::<Sqlite>::new("INSERT INTO messages_fts(server_id, conversation_id, text) ");
    insert_qb.push_values(searchable, |mut b, (row, text)| {
        b.push_bind(&row.message.server_id)
            .push_bind(&row.message.conversation_id)
            .push_bind(text);
    });
    insert_qb
        .build()
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

async fn merge_reactions_to_server_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
    old_server_id: &str,
    target_server_id: &str,
    target_conversation_id: &str,
) -> Result<()> {
    let old_server_id = old_server_id.trim();
    let target_server_id = target_server_id.trim();
    if old_server_id.is_empty() || target_server_id.is_empty() || old_server_id == target_server_id
    {
        return Ok(());
    }

    let now = now_ms_i64();
    sqlx::query(
        r#"INSERT OR REPLACE INTO message_reactions (
               message_server_id, conversation_id, emoji, user_id, created_at, updated_at
           )
           SELECT ?, ?, emoji, user_id, created_at, ?
           FROM message_reactions
           WHERE message_server_id = ?"#,
    )
    .bind(target_server_id)
    .bind(target_conversation_id)
    .bind(now)
    .bind(old_server_id)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    sqlx::query("DELETE FROM message_reactions WHERE message_server_id = ?")
        .bind(old_server_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

async fn remove_conflicting_client_msg_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    client_msg_id: &str,
    target_server_id: &str,
    target_conversation_id: &str,
) -> Result<()> {
    let client_msg_id = client_msg_id.trim();
    let target_server_id = target_server_id.trim();
    if client_msg_id.is_empty() || target_server_id.is_empty() {
        return Ok(());
    }

    let rows = sqlx::query(
        r#"SELECT server_id
           FROM messages
           WHERE client_msg_id = ? AND server_id <> ?"#,
    )
    .bind(client_msg_id)
    .bind(target_server_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    for row in rows {
        let old_server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        delete_message_fts_by_server_id_tx(tx, &old_server_id).await?;
        merge_reactions_to_server_id_tx(
            tx,
            &old_server_id,
            target_server_id,
            target_conversation_id,
        )
        .await?;
    }

    sqlx::query("DELETE FROM messages WHERE client_msg_id = ? AND server_id <> ?")
        .bind(client_msg_id)
        .bind(target_server_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

async fn insert_message_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[MessagePersistRow<'_>],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut merged_attributes_json = Vec::with_capacity(rows.len());
    for row in rows {
        let incoming = parse_extra(Some(&row.attributes_json));
        let existing_attributes = existing_message_attributes_tx(tx, row).await?;
        let merged = match existing_attributes {
            Some(raw) => merge_message_event_attributes(incoming, parse_extra(Some(&raw))),
            None => incoming,
        };
        merged_attributes_json.push(extra_to_json(&merged));
    }

    let mut qb = QueryBuilder::<Sqlite>::new(
        r#"INSERT OR REPLACE INTO messages (
           server_id, conversation_id, client_msg_id, sender_id, source, conversation_seq, created_at, client_created_at,
           conversation_type, message_type, channel_id, sender_name, sender_avatar,
           sender_display_name, encoded_content, status,
           retention_policy, retention_state,
           is_read, is_recalled, is_edited,
           reply_to, quote_preview, mention_users, mention_all, attributes, extensions, version, updated_at, text,
           sending, failed, is_local, sort_ts)
        "#,
    );
    qb.push_values(rows.iter().enumerate(), |mut b, (index, row)| {
        let m = row.message;
        b.push_bind(&m.server_id)
            .push_bind(&m.conversation_id)
            .push_bind(&m.client_msg_id)
            .push_bind(&m.sender_id)
            .push_bind(m.source)
            .push_bind(m.conversation_seq as i64)
            .push_bind(m.created_at as i64)
            .push_bind(m.client_created_at as i64)
            .push_bind(m.conversation_type)
            .push_bind(m.message_type)
            .push_bind(&m.channel_id)
            .push_bind(&m.sender_name)
            .push_bind(&m.sender_avatar)
            .push_bind(&m.sender_display_name)
            .push_bind(&m.encoded_content)
            .push_bind(m.status)
            .push_bind(encode_optional_proto(&m.retention_policy))
            .push_bind(encode_optional_proto(&m.retention_state))
            .push_bind(if m.is_read { 1i32 } else { 0 })
            .push_bind(if m.is_recalled { 1i32 } else { 0 })
            .push_bind(if m.is_edited { 1i32 } else { 0 })
            .push_bind(&m.reply_to)
            .push_bind(&m.quote_preview)
            .push_bind(&row.mention_users_json)
            .push_bind(if m.mention_all { 1i32 } else { 0 })
            .push_bind(&merged_attributes_json[index])
            .push_bind(&row.extensions_json)
            .push_bind(m.version as i64)
            .push_bind(m.updated_at as i64)
            .push_bind(row.text.as_deref())
            .push_bind(if m.local_state.sending { 1i32 } else { 0 })
            .push_bind(if m.local_state.failed { 1i32 } else { 0 })
            .push_bind(if m.local_state.is_local { 1i32 } else { 0 })
            .push_bind(effective_sort_ts_for_persist(m));
    });
    qb.build().execute(&mut **tx).await.map_err(sqlx_err)?;
    Ok(())
}

async fn existing_message_attributes_tx(
    tx: &mut Transaction<'_, Sqlite>,
    row: &MessagePersistRow<'_>,
) -> Result<Option<String>> {
    let server_id = row.message.server_id.trim();
    let client_msg_id = row.message.client_msg_id.trim();
    let existing = if !server_id.is_empty() && !client_msg_id.is_empty() {
        sqlx::query(
            r#"SELECT attributes FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(server_id)
        .bind(client_msg_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(sqlx_err)?
    } else if !server_id.is_empty() {
        sqlx::query("SELECT attributes FROM messages WHERE server_id = ? LIMIT 1")
            .bind(server_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(sqlx_err)?
    } else if !client_msg_id.is_empty() {
        sqlx::query("SELECT attributes FROM messages WHERE client_msg_id = ? LIMIT 1")
            .bind(client_msg_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(sqlx_err)?
    } else {
        None
    };

    if let Some(row) = existing {
        let raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
        Ok(raw)
    } else {
        Ok(None)
    }
}

async fn delete_message_fts_by_message_id_tx(
    tx: &mut Transaction<'_, Sqlite>,
    message_id: &str,
) -> Result<()> {
    let rows = sqlx::query(
        r#"SELECT server_id
           FROM messages
           WHERE server_id = ? OR client_msg_id = ?"#,
    )
    .bind(message_id)
    .bind(message_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    for row in rows {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        delete_message_fts_by_server_id_tx(tx, &server_id).await?;
    }
    Ok(())
}

fn conversation_projection_ts(message: &IMMessage) -> i64 {
    if message.conversation_seq > 0 {
        let server_time = if message.created_at > 0 {
            message.created_at
        } else if message.client_created_at > 0 {
            message.client_created_at
        } else {
            message.timeline_sort_ts()
        };
        return u64_to_i64_saturating(server_time);
    }

    let merged = message.timeline_sort_ts();
    if merged > 0 {
        u64_to_i64_saturating(merged)
    } else {
        effective_sort_ts_for_persist(message)
    }
}

fn should_replace_conversation_projection(prev: &IMMessage, candidate: &IMMessage) -> bool {
    match (prev.conversation_seq > 0, candidate.conversation_seq > 0) {
        (true, true) => {
            candidate.conversation_seq > prev.conversation_seq
                || (candidate.conversation_seq == prev.conversation_seq
                    && effective_sort_ts_for_persist(candidate)
                        >= effective_sort_ts_for_persist(prev))
        }
        _ => {
            let candidate_sort = effective_sort_ts_for_persist(candidate);
            let prev_sort = effective_sort_ts_for_persist(prev);
            candidate_sort > prev_sort
                || (candidate_sort == prev_sort
                    && candidate.conversation_seq >= prev.conversation_seq)
        }
    }
}

fn search_effective_time_sql(prefix: &str) -> String {
    format!(
        "COALESCE(NULLIF({prefix}.created_at, 0), NULLIF({prefix}.client_created_at, 0), NULLIF({prefix}.sort_ts, 0), 0)"
    )
}

fn message_type_values_for_search(kinds: &[MessageSearchKind]) -> Vec<i32> {
    let mut values = Vec::new();
    for kind in kinds {
        match kind {
            MessageSearchKind::Message => return Vec::new(),
            MessageSearchKind::Text => {
                values.push(MessageType::Text as i32);
                values.push(MessageType::RichText as i32);
                values.push(MessageType::Quote as i32);
            }
            MessageSearchKind::Media => {
                values.push(MessageType::Image as i32);
                values.push(MessageType::Video as i32);
                values.push(MessageType::Audio as i32);
                values.push(MessageType::File as i32);
                values.push(MessageType::ImageGroup as i32);
            }
            MessageSearchKind::Image => {
                values.push(MessageType::Image as i32);
                values.push(MessageType::ImageGroup as i32);
            }
            MessageSearchKind::Video => values.push(MessageType::Video as i32),
            MessageSearchKind::Audio => values.push(MessageType::Audio as i32),
            MessageSearchKind::File => values.push(MessageType::File as i32),
        }
    }
    values.sort_unstable();
    values.dedup();
    values
}

/// 从 `MessageContent` 字节解码为 `messages.text` 的标准 preview 载荷。
fn text_for_sqlite_from_content_bytes(bytes: &[u8]) -> Option<String> {
    preview_for_content_bytes(bytes)
}

fn preview_for_content_bytes(bytes: &[u8]) -> Option<String> {
    decode_content_bytes(bytes)
        .ok()
        .and_then(|decoded| decoded_content_to_elem(&decoded))
        .and_then(|elem| {
            let payload = elem_preview_storage_payload(&elem);
            if payload.is_empty_for_last_preview() {
                return None;
            }
            serde_json::to_string(&payload).ok()
        })
}

fn parse_extensions(s: Option<&str>) -> HashMap<String, Vec<u8>> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    let map: HashMap<String, String> = match serde_json::from_str(s) {
        Ok(m) => m,
        Err(_) => return HashMap::new(),
    };
    map.into_iter()
        .filter_map(|(k, v)| BASE64_STANDARD.decode(&v).ok().map(|b| (k, b)))
        .collect()
}

const MESSAGE_SELECT_COLS: &str = r#"server_id, conversation_id, client_msg_id, sender_id, source,
    conversation_seq, created_at, client_created_at, conversation_type, message_type, channel_id,
    sender_name, sender_avatar, sender_display_name, encoded_content, status,
    retention_policy, retention_state,
    is_read, is_recalled, is_edited,
    reply_to, quote_preview, mention_users, mention_all, attributes, extensions, version, updated_at, text,
    sending, failed, is_local, sort_ts"#;

pub struct SqliteMessageRepo {
    pool: SqlitePool,
}

fn parse_conversation_ext_json(s: Option<&str>) -> HashMap<String, String> {
    let s = match s {
        Some(x) if !x.is_empty() => x,
        _ => return HashMap::new(),
    };
    serde_json::from_str(s).unwrap_or_default()
}

impl SqliteMessageRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn local_cleared_floor(&self, conversation_id: &str) -> Result<u64> {
        let row: Option<(Option<String>, i64)> =
            sqlx::query_as(
                "SELECT ext, COALESCE(visible_after_seq, 0) FROM conversations WHERE conversation_id = ? LIMIT 1",
            )
                .bind(conversation_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let Some((ext, visible_after_seq)) = row else {
            return Ok(0);
        };
        Ok(
            local_cleared_through_seq(&parse_conversation_ext_json(ext.as_deref()))
                .max(visible_after_seq.max(0) as u64),
        )
    }

    fn row_to_immessage(&self, row: &sqlx::sqlite::SqliteRow) -> Result<IMMessage> {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
        let client_msg_id: String = row.try_get("client_msg_id").map_err(sqlx_err)?;
        let sender_id: String = row.try_get("sender_id").map_err(sqlx_err)?;
        let source: i32 = row.try_get("source").map_err(sqlx_err)?;
        let conversation_seq: i64 = row.try_get("conversation_seq").map_err(sqlx_err)?;
        let created_at: i64 = row.try_get("created_at").map_err(sqlx_err)?;
        let client_created_at: i64 = row.try_get("client_created_at").map_err(sqlx_err)?;
        let conversation_type: i32 = row.try_get("conversation_type").map_err(sqlx_err)?;
        let message_type: i32 = row.try_get("message_type").map_err(sqlx_err)?;
        let channel_id: String = row
            .try_get::<Option<String>, _>("channel_id")
            .map_err(sqlx_err)?
            .unwrap_or_default();
        let sender_name: String = row.try_get("sender_name").map_err(sqlx_err)?;
        let sender_avatar: String = row.try_get("sender_avatar").map_err(sqlx_err)?;
        let sender_display_name: String = row.try_get("sender_display_name").map_err(sqlx_err)?;
        let encoded_content: Vec<u8> = row.try_get("encoded_content").map_err(sqlx_err)?;
        let status: i32 = row.try_get("status").map_err(sqlx_err)?;
        let retention_policy_bytes: Vec<u8> = row.try_get("retention_policy").map_err(sqlx_err)?;
        let retention_state_bytes: Vec<u8> = row.try_get("retention_state").map_err(sqlx_err)?;
        let is_read: i32 = row.try_get("is_read").map_err(sqlx_err)?;
        let is_recalled: i32 = row.try_get("is_recalled").map_err(sqlx_err)?;
        let is_edited: i32 = row.try_get("is_edited").map_err(sqlx_err)?;
        let reply_to: Option<String> = row.try_get("reply_to").map_err(sqlx_err)?;
        let quote_preview: Option<String> = row.try_get("quote_preview").map_err(sqlx_err)?;
        let mention_users_json: Option<String> = row.try_get("mention_users").map_err(sqlx_err)?;
        let mention_all: i32 = row.try_get("mention_all").map_err(sqlx_err)?;
        let extra_json: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
        let extensions_json: Option<String> = row.try_get("extensions").map_err(sqlx_err)?;
        let version: i64 = row.try_get("version").map_err(sqlx_err)?;
        let updated_at: i64 = row.try_get("updated_at").map_err(sqlx_err)?;
        let sending: i32 = row.try_get("sending").map_err(sqlx_err)?;
        let failed: i32 = row.try_get("failed").map_err(sqlx_err)?;
        let is_local: i32 = row.try_get("is_local").map_err(sqlx_err)?;
        let sort_ts: i64 = row.try_get("sort_ts").map_err(sqlx_err)?;
        let text_col: Option<String> = row.try_get("text").map_err(sqlx_err)?;

        let mut attributes = parse_extra(extra_json.as_deref());
        let mut content = decode_content_bytes(&encoded_content)
            .ok()
            .and_then(|decoded| decoded_content_to_elem(&decoded));
        if content.is_none()
            && message_type == MessageType::Text as i32
            && let Some(ref t) = text_col
        {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                attributes
                    .entry("contentText".to_string())
                    .or_insert_with(|| trimmed.to_string());
                content = Some(Elem::Text(TextElem {
                    text: trimmed.to_string(),
                    mentions: vec![],
                }));
            }
        }

        let mut ts_u = created_at.max(0) as u64;
        let mut cts_u = client_created_at.max(0) as u64;
        let sort_u = sort_ts.max(0) as u64;
        // 待发/旧数据可能未写 `created_at`，但 `sort_ts` 已在落库时规范化（见 `effective_sort_ts_for_persist`），
        // 读出时回填给前端时间排序，避免 0 被当成「最早」。
        if ts_u == 0 && cts_u == 0 && sort_u > 0 {
            ts_u = sort_u;
            cts_u = sort_u;
        }

        Ok(IMMessage {
            server_id,
            client_msg_id,
            conversation_id,
            conversation_type,
            channel_id,
            sender_id,
            source,
            conversation_seq: conversation_seq.max(0) as u64,
            created_at: ts_u,
            client_created_at: cts_u,
            message_type,
            content,
            encoded_content,
            text_preview: text_col.unwrap_or_default(),
            sender_name,
            sender_avatar,
            sender_display_name,
            reply_to,
            quote_preview,
            status,
            retention_policy: decode_optional_proto::<MessageRetentionPolicy>(
                retention_policy_bytes,
            ),
            retention_state: decode_optional_proto::<MessageRetentionState>(retention_state_bytes),
            is_read: is_read != 0,
            is_recalled: is_recalled != 0,
            is_edited: is_edited != 0,
            mention_users: parse_mention_users(mention_users_json.as_deref()),
            mention_all: mention_all != 0,
            offline_push_info: None,
            reactions: parse_reactions_from_attributes(&attributes),
            attributes,
            extensions: parse_extensions(extensions_json.as_deref()),
            version: version.max(0) as u64,
            updated_at: updated_at.max(0) as u64,
            local_state: MessageLocalState {
                sending: sending != 0,
                failed: failed != 0,
                is_local: is_local != 0,
                uploading: false,
                upload_progress: 0,
                sort_ts: sort_ts.max(0) as u64,
            },
        })
    }
}

#[async_trait]
impl MessageReader for SqliteMessageRepo {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let row = sqlx::query(&format!(
            "SELECT {} FROM messages WHERE server_id = ?",
            MESSAGE_SELECT_COLS
        ))
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        row.map(|r| self.row_to_immessage(&r)).transpose()
    }

    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        let row = sqlx::query(&format!(
            r#"SELECT {} FROM messages
               WHERE client_msg_id = ?
               ORDER BY
                 CASE WHEN server_id = client_msg_id THEN 1 ELSE 0 END ASC,
                 conversation_seq DESC,
                 sort_ts DESC,
                 updated_at DESC
               LIMIT 1"#,
            MESSAGE_SELECT_COLS
        ))
        .bind(client_msg_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        row.map(|r| self.row_to_immessage(&r)).transpose()
    }

    async fn get_by_client_msg_ids(&self, client_msg_ids: &[String]) -> Result<Vec<IMMessage>> {
        if client_msg_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(client_msg_ids.len());
        let mut seen = HashSet::with_capacity(client_msg_ids.len());
        // 单次 `IN (...)` 替代逐条查询；分块以兼容 SQLite 较保守的绑定参数上限。
        for chunk in client_msg_ids.chunks(500) {
            let placeholders = std::iter::repeat("?")
                .take(chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                r#"SELECT {} FROM messages
                   WHERE client_msg_id IN ({})
                   ORDER BY
                     client_msg_id ASC,
                     CASE WHEN server_id = client_msg_id THEN 1 ELSE 0 END ASC,
                     conversation_seq DESC,
                     sort_ts DESC,
                     updated_at DESC"#,
                MESSAGE_SELECT_COLS, placeholders
            );
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            let rows = query
                .fetch_all(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
            for row in &rows {
                let message = self.row_to_immessage(row)?;
                if seen.insert(message.client_msg_id.clone()) {
                    out.push(message);
                }
            }
        }
        Ok(out)
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        identity_repair::repair_single_chat_message_alias_for_conversation(
            &self.pool,
            conversation_id,
        )
        .await?;
        // 与 `before_seq_for_sqlite` 一致：`0` / `>= i64::MAX` 表示「最新一页」游标。
        let is_latest_window = before_seq == 0 || before_seq >= i64::MAX as u64;
        let bound = before_seq_for_sqlite(before_seq);
        let cleared_floor = self.local_cleared_floor(conversation_id).await?;

        let rows = if is_latest_window {
            // 本地待 ACK 消息保持在最新窗口顶部；已分配 conversation_seq 的消息以服务端 seq 为权威。
            // 不能再用 max(sort_ts, created_at, client_created_at) 作为主排序，否则设备时钟偏移会让 ACK 后的旧消息长期顶在首屏。
            let sql = if cleared_floor > 0 {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND conversation_seq < ? AND (conversation_seq = 0 OR conversation_seq > ?)
                   ORDER BY
                     CASE WHEN conversation_seq = 0 AND is_local = 1 THEN 1 ELSE 0 END DESC,
                     CASE WHEN conversation_seq > 0 THEN conversation_seq ELSE 0 END DESC,
                     CASE
                       WHEN conversation_seq > 0 THEN COALESCE(NULLIF(created_at, 0), NULLIF(client_created_at, 0), NULLIF(sort_ts, 0), 0)
                       ELSE max(max(sort_ts, created_at), client_created_at)
                     END DESC
                   LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            } else {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND conversation_seq < ?
                   ORDER BY
                     CASE WHEN conversation_seq = 0 AND is_local = 1 THEN 1 ELSE 0 END DESC,
                     CASE WHEN conversation_seq > 0 THEN conversation_seq ELSE 0 END DESC,
                     CASE
                       WHEN conversation_seq > 0 THEN COALESCE(NULLIF(created_at, 0), NULLIF(client_created_at, 0), NULLIF(sort_ts, 0), 0)
                       ELSE max(max(sort_ts, created_at), client_created_at)
                     END DESC
                   LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            };
            let mut q = sqlx::query(&sql).bind(conversation_id).bind(bound);
            if cleared_floor > 0 {
                q = q.bind(cleared_floor as i64);
            }
            q.bind(limit as i32).fetch_all(&self.pool).await
        } else {
            // 翻页只拉已分配 conversation_seq 的历史消息，避免 `conversation_seq == 0` 的待发送行在第二页重复出现。
            let sql = if cleared_floor > 0 {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND conversation_seq > 0 AND conversation_seq < ? AND conversation_seq > ?
                   ORDER BY conversation_seq DESC LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            } else {
                format!(
                    r#"SELECT {} FROM messages
                   WHERE conversation_id = ? AND conversation_seq > 0 AND conversation_seq < ?
                   ORDER BY conversation_seq DESC LIMIT ?"#,
                    MESSAGE_SELECT_COLS
                )
            };
            let mut q = sqlx::query(&sql).bind(conversation_id).bind(bound);
            if cleared_floor > 0 {
                q = q.bind(cleared_floor as i64);
            }
            q.bind(limit as i32).fetch_all(&self.pool).await
        }
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_immessage(&row)?);
        }
        Ok(out)
    }

    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.search_by_query(&MessageSearchQuery::text(keyword, limit))
            .await
    }

    async fn search_by_query(&self, query: &MessageSearchQuery) -> Result<Vec<IMMessage>> {
        let effective_time = search_effective_time_sql("messages");
        let mut sql = format!("SELECT {} FROM messages WHERE 1 = 1", MESSAGE_SELECT_COLS);
        sql.push_str(" AND (conversation_seq = 0 OR conversation_seq > COALESCE((SELECT visible_after_seq FROM conversations WHERE conversation_id = messages.conversation_id LIMIT 1), 0))");
        if !query.include_recalled {
            sql.push_str(" AND COALESCE(is_recalled, 0) = 0");
        }

        let mut qb = QueryBuilder::<Sqlite>::new(sql);
        if let Some(conversation_id) = query
            .conversation_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(" AND conversation_id = ");
            qb.push_bind(conversation_id);
        }
        if let Some(sender_id) = query
            .sender_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            qb.push(" AND sender_id = ");
            qb.push_bind(sender_id);
        }
        if let Some(keyword) = query.normalized_keyword() {
            let Some(search_plan) = sqlite_keyword_search(&keyword) else {
                return Ok(Vec::new());
            };
            match search_plan {
                SqliteKeywordSearch::Fts(fts_query) => {
                    qb.push(
                        " AND server_id IN (SELECT server_id FROM messages_fts WHERE messages_fts MATCH ",
                    );
                    qb.push_bind(fts_query);
                    qb.push(")");
                }
                SqliteKeywordSearch::ContentLike(like) => {
                    qb.push(" AND (LOWER(COALESCE(text, '')) LIKE ");
                    qb.push_bind(like.clone());
                    qb.push(r#" ESCAPE '\' OR LOWER(COALESCE(CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.contentText') ELSE '' END, '')) LIKE "#);
                    qb.push_bind(like.clone());
                    qb.push(r#" ESCAPE '\' OR server_id IN (SELECT server_id FROM messages_fts WHERE LOWER(COALESCE(text, '')) LIKE "#);
                    qb.push_bind(like);
                    qb.push(r#" ESCAPE '\'))"#);
                }
            }
        }
        if let Some(from_time) = query.from_time {
            qb.push(" AND ");
            qb.push(&effective_time);
            qb.push(" >= ");
            qb.push_bind(from_time.min(i64::MAX as u64) as i64);
        }
        if let Some(to_time) = query.to_time {
            qb.push(" AND ");
            qb.push(&effective_time);
            qb.push(" <= ");
            qb.push_bind(to_time.min(i64::MAX as u64) as i64);
        }

        let message_types = message_type_values_for_search(&query.kinds);
        if !message_types.is_empty() {
            qb.push(" AND message_type IN (");
            let mut separated = qb.separated(", ");
            for value in message_types {
                separated.push_bind(value);
            }
            separated.push_unseparated(")");
        }

        qb.push(" ORDER BY ");
        qb.push(&effective_time);
        qb.push(" DESC, conversation_seq DESC LIMIT ");
        qb.push_bind(query.normalized_limit() as i32);

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.row_to_immessage(&row)?);
        }
        Ok(out)
    }

    async fn search_in_conversation(
        &self,
        conversation_id: &str,
        keyword: &str,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            return Ok(Vec::new());
        }
        self.search_by_query(&MessageSearchQuery::in_conversation(
            conversation_id,
            keyword,
            limit,
        ))
        .await
    }
}

fn extensions_to_json(ext: &HashMap<String, Vec<u8>>) -> String {
    if ext.is_empty() {
        return String::new();
    }
    let map: HashMap<String, String> = ext
        .iter()
        .map(|(k, v)| (k.clone(), BASE64_STANDARD.encode(v)))
        .collect();
    serde_json::to_string(&map).unwrap_or_default()
}

fn now_ms_i64() -> i64 {
    now_millis().min(i64::MAX as u64) as i64
}

async fn refresh_reactions_json_snapshot(pool: &SqlitePool, message_id: &str) -> Result<()> {
    let id = message_id.trim();
    if id.is_empty() {
        return Ok(());
    }
    let message_row = sqlx::query(
        r#"SELECT server_id, attributes FROM messages
           WHERE server_id = ? OR client_msg_id = ?
           LIMIT 1"#,
    )
    .bind(id)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(sqlx_err)?;
    let Some(row) = message_row else {
        return Ok(());
    };
    let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
    if server_id.trim().is_empty() {
        return Ok(());
    }

    let reaction_rows = sqlx::query(
        r#"SELECT emoji, user_id
           FROM message_reactions
           WHERE message_server_id = ?
           ORDER BY updated_at ASC"#,
    )
    .bind(server_id.trim())
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for rr in reaction_rows {
        let emoji: String = rr.try_get("emoji").map_err(sqlx_err)?;
        let user_id: String = rr.try_get("user_id").map_err(sqlx_err)?;
        if emoji.trim().is_empty() || user_id.trim().is_empty() {
            continue;
        }
        grouped
            .entry(emoji.trim().to_string())
            .or_default()
            .push(user_id.trim().to_string());
    }
    let mut reactions: Vec<ReactionEntry> = grouped
        .into_iter()
        .map(|(emoji, user_ids)| ReactionEntry {
            emoji,
            count: user_ids.len() as u32,
            user_ids,
        })
        .collect();
    reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
    debug!(
        message_id = %id,
        server_id = %server_id,
        reaction_group_count = reactions.len(),
        "refresh_reactions_json_snapshot"
    );

    let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
    let mut attributes = parse_extra(extra_raw.as_deref());
    if reactions.is_empty() {
        attributes.remove("reactionsJson");
    } else if let Ok(raw) = serde_json::to_string(&reactions) {
        attributes.insert("reactionsJson".to_string(), raw);
    }
    sqlx::query("UPDATE messages SET attributes = ? WHERE server_id = ?")
        .bind(extra_to_json(&attributes))
        .bind(server_id)
        .execute(pool)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}

fn conversation_display_name_from_message(message: &IMMessage) -> String {
    if !message.channel_id.trim().is_empty() {
        return message.channel_id.trim().to_string();
    }
    if !message.sender_name.trim().is_empty() {
        return message.sender_name.trim().to_string();
    }
    if !message.sender_id.trim().is_empty() {
        return message.sender_id.trim().to_string();
    }
    message.conversation_id.trim().to_string()
}

async fn upsert_conversation_snapshot_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &IMMessage,
) -> Result<()> {
    let conversation_id = message.conversation_id.trim();
    if conversation_id.is_empty() {
        return Ok(());
    }

    let conv_type = ConversationType::from_proto_int(message.conversation_type);
    let conversation_type = conv_type.to_proto_int();
    let display_name = conversation_display_name_from_message(message);
    let business_type = if conv_type.is_single_chat_conversation() {
        "single"
    } else {
        "chat"
    };
    let last_message_id = if message.server_id.trim().is_empty() {
        message.client_msg_id.trim()
    } else {
        message.server_id.trim()
    };
    let last_sender_id = message.sender_id.trim();
    let last_message_at = conversation_projection_ts(message);
    let preview = message_preview_for_storage(message).unwrap_or_default();
    let max_seq = message.conversation_seq as i64;
    let now = now_ms_i64();
    let created_at = if last_message_at > 0 {
        last_message_at
    } else {
        now
    };
    let updated_at = if last_message_at > 0 {
        last_message_at
    } else {
        now
    };

    sqlx::query(
        r#"INSERT INTO conversations (
               conversation_id, conversation_type, business_type, channel_id, members_count,
               display_name, avatar_url, remark, description,
               last_message_id, last_sender_id, last_message_at, last_message_preview,
               last_sender_nickname, last_sender_avatar_url,
               unread_count, last_read_seq, max_seq,
               is_pinned, is_muted, is_archived,
               version, updated_at, created_at, updated_at_ts,
               ext, draft, mention_count, mention_me, badge, role
           ) VALUES (
               ?, ?, ?, ?, 0,
               ?, '', NULL, NULL,
               ?, ?, ?, ?,
               '', '',
               0, 0, ?,
               0, 0, 0,
               0, ?, ?, ?,
               '', NULL, 0, 0, NULL, NULL
           )
           ON CONFLICT(conversation_id) DO UPDATE SET
               conversation_type = CASE
                   WHEN conversations.conversation_type = 0 AND excluded.conversation_type != 0 THEN excluded.conversation_type
                   ELSE conversations.conversation_type
               END,
               business_type = CASE
                   WHEN conversations.business_type = '' THEN excluded.business_type
                   ELSE conversations.business_type
               END,
               channel_id = CASE
                   WHEN conversations.channel_id = '' THEN excluded.channel_id
                   ELSE conversations.channel_id
               END,
               display_name = CASE
                   WHEN conversations.display_name = '' THEN excluded.display_name
                   ELSE conversations.display_name
               END,
               avatar_url = CASE
                   WHEN conversations.avatar_url = '' THEN excluded.avatar_url
               ELSE conversations.avatar_url
               END,
               last_message_id = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_message_id
                   ELSE conversations.last_message_id
               END,
               last_sender_id = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_sender_id
                   ELSE conversations.last_sender_id
               END,
               last_message_at = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_message_at
                   ELSE conversations.last_message_at
               END,
               last_message_preview = CASE
                   WHEN
                       (COALESCE(excluded.max_seq, 0) > COALESCE(conversations.max_seq, 0))
                    OR (COALESCE(excluded.max_seq, 0) > 0
                        AND COALESCE(excluded.max_seq, 0) = COALESCE(conversations.max_seq, 0)
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                    OR (COALESCE(excluded.max_seq, 0) = 0
                        AND COALESCE(excluded.last_message_at, 0) >= COALESCE(conversations.last_message_at, 0))
                   THEN excluded.last_message_preview
                   ELSE conversations.last_message_preview
               END,
               max_seq = MAX(COALESCE(conversations.max_seq, 0), COALESCE(excluded.max_seq, 0)),
               updated_at = MAX(COALESCE(conversations.updated_at, 0), COALESCE(excluded.updated_at, 0)),
               updated_at_ts = MAX(COALESCE(conversations.updated_at_ts, 0), COALESCE(excluded.updated_at_ts, 0))
        "#,
    )
    .bind(conversation_id)
    .bind(conversation_type)
    .bind(business_type)
    .bind(&message.channel_id)
    .bind(&display_name)
    .bind(last_message_id)
    .bind(last_sender_id)
    .bind(last_message_at)
    .bind(&preview)
    .bind(max_seq)
    .bind(updated_at)
    .bind(created_at)
    .bind(updated_at)
    .execute(&mut **tx)
    .await
    .map_err(sqlx_err)?;
    Ok(())
}

async fn replace_reaction_snapshot_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &IMMessage,
) -> Result<()> {
    if message.server_id.is_empty() {
        return Ok(());
    }
    let has_snapshot =
        has_reaction_snapshot_in_attributes(&message.attributes) || !message.reactions.is_empty();
    if !has_snapshot {
        // 下行消息通常不携带 reactions 快照，不能把已落库的反应误删。
        return Ok(());
    }
    sqlx::query("DELETE FROM message_reactions WHERE message_server_id = ?")
        .bind(&message.server_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    if message.reactions.is_empty() {
        return Ok(());
    }
    let now = now_ms_i64();
    for reaction in &message.reactions {
        if reaction.emoji.trim().is_empty() {
            continue;
        }
        for uid in &reaction.user_ids {
            if uid.trim().is_empty() {
                continue;
            }
            sqlx::query(
                r#"INSERT OR REPLACE INTO message_reactions
                   (message_server_id, conversation_id, emoji, user_id, created_at, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(&message.server_id)
            .bind(&message.conversation_id)
            .bind(&reaction.emoji)
            .bind(uid)
            .bind(now)
            .bind(now)
            .execute(&mut **tx)
            .await
            .map_err(sqlx_err)?;
        }
    }
    Ok(())
}

async fn replace_reaction_snapshot_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[MessagePersistRow<'_>],
) -> Result<()> {
    let snapshot_rows = rows
        .iter()
        .filter(|row| {
            let message = row.message;
            !message.server_id.is_empty()
                && (has_reaction_snapshot_in_attributes(&message.attributes)
                    || !message.reactions.is_empty())
        })
        .collect::<Vec<_>>();
    if snapshot_rows.is_empty() {
        return Ok(());
    }

    let mut delete_qb =
        QueryBuilder::<Sqlite>::new("DELETE FROM message_reactions WHERE message_server_id IN (");
    {
        let mut separated = delete_qb.separated(", ");
        for row in &snapshot_rows {
            separated.push_bind(&row.message.server_id);
        }
    }
    delete_qb.push(")");
    delete_qb
        .build()
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;

    let now = now_ms_i64();
    let mut entries = Vec::new();
    for row in snapshot_rows {
        let message = row.message;
        for reaction in &message.reactions {
            if reaction.emoji.trim().is_empty() {
                continue;
            }
            for uid in &reaction.user_ids {
                if uid.trim().is_empty() {
                    continue;
                }
                entries.push((
                    message.server_id.as_str(),
                    message.conversation_id.as_str(),
                    reaction.emoji.as_str(),
                    uid.as_str(),
                ));
            }
        }
    }

    for chunk in entries.chunks(MESSAGE_REACTION_INSERT_CHUNK_SIZE) {
        let mut insert_qb = QueryBuilder::<Sqlite>::new(
            r#"INSERT OR REPLACE INTO message_reactions
               (message_server_id, conversation_id, emoji, user_id, created_at, updated_at) "#,
        );
        insert_qb.push_values(chunk, |mut b, entry| {
            b.push_bind(entry.0)
                .push_bind(entry.1)
                .push_bind(entry.2)
                .push_bind(entry.3)
                .push_bind(now)
                .push_bind(now);
        });
        insert_qb
            .build()
            .execute(&mut **tx)
            .await
            .map_err(sqlx_err)?;
    }

    Ok(())
}

async fn refresh_conversation_snapshot_after_message_delete_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    conversation_id: &str,
) -> Result<()> {
    if conversation_id.trim().is_empty() {
        return Ok(());
    }

    let row = sqlx::query(
        r#"SELECT m.server_id, m.client_msg_id, m.sender_id,
                  CASE
                    WHEN m.conversation_seq > 0 THEN COALESCE(NULLIF(m.created_at, 0), NULLIF(m.client_created_at, 0), NULLIF(m.sort_ts, 0), 0)
                    ELSE max(max(COALESCE(m.sort_ts, 0), COALESCE(m.created_at, 0)), COALESCE(m.client_created_at, 0))
                  END AS message_at,
                  text
           FROM messages m
           LEFT JOIN conversations c ON c.conversation_id = m.conversation_id
           WHERE m.conversation_id = ?
             AND TRIM(COALESCE(m.text, '')) != ''
             AND (
                 m.conversation_seq = 0
                 OR m.conversation_seq > COALESCE(c.visible_after_seq, 0)
             )
           ORDER BY
             CASE
               WHEN m.conversation_seq = 0
                AND max(max(COALESCE(m.sort_ts, 0), COALESCE(m.created_at, 0)), COALESCE(m.client_created_at, 0)) >
                    COALESCE((
                      SELECT max(COALESCE(NULLIF(s.created_at, 0), NULLIF(s.client_created_at, 0), NULLIF(s.sort_ts, 0), 0))
                      FROM messages s
                      WHERE s.conversation_id = m.conversation_id
                        AND s.conversation_seq > COALESCE(c.visible_after_seq, 0)
                        AND s.conversation_seq > 0
                        AND TRIM(COALESCE(s.text, '')) != ''
                    ), 0)
               THEN 1 ELSE 0
             END DESC,
             CASE WHEN m.conversation_seq > 0 THEN m.conversation_seq ELSE 0 END DESC,
             CASE
               WHEN m.conversation_seq > 0 THEN COALESCE(NULLIF(m.created_at, 0), NULLIF(m.client_created_at, 0), NULLIF(m.sort_ts, 0), 0)
               ELSE max(max(COALESCE(m.sort_ts, 0), COALESCE(m.created_at, 0)), COALESCE(m.client_created_at, 0))
             END DESC
           LIMIT 1"#,
    )
    .bind(conversation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(sqlx_err)?;

    if let Some(row) = row {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        let client_msg_id: String = row.try_get("client_msg_id").map_err(sqlx_err)?;
        let sender_id: String = row.try_get("sender_id").map_err(sqlx_err)?;
        let message_at: i64 = row.try_get("message_at").map_err(sqlx_err)?;
        let text: Option<String> = row.try_get("text").map_err(sqlx_err)?;
        let message_id = if server_id.trim().is_empty() {
            client_msg_id
        } else {
            server_id
        };
        sqlx::query(
            r#"UPDATE conversations
               SET last_message_id = ?,
                   last_sender_id = ?,
                   last_message_at = ?,
                   last_message_preview = ?,
                   updated_at = MAX(COALESCE(updated_at, 0), ?),
                   updated_at_ts = MAX(COALESCE(updated_at_ts, 0), ?)
               WHERE conversation_id = ?"#,
        )
        .bind(message_id)
        .bind(sender_id)
        .bind(message_at)
        .bind(text.as_deref())
        .bind(message_at)
        .bind(message_at)
        .bind(conversation_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    } else {
        sqlx::query(
            r#"UPDATE conversations
               SET last_message_id = NULL,
                   last_sender_id = NULL,
                   last_message_at = NULL,
                   last_message_preview = NULL
               WHERE conversation_id = ?"#,
        )
        .bind(conversation_id)
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    }

    Ok(())
}

#[async_trait]
impl MessageWriter for SqliteMessageRepo {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()> {
        let mut cleared_floors: HashMap<String, u64> = HashMap::new();
        let mut persistable: Vec<&IMMessage> = Vec::new();
        for m in messages {
            let cid = m.conversation_id.trim();
            if cid.is_empty() {
                continue;
            }
            let floor = if let Some(v) = cleared_floors.get(cid) {
                *v
            } else {
                let v = self.local_cleared_floor(cid).await?;
                cleared_floors.insert(cid.to_string(), v);
                v
            };
            if message_visible_after_clear(m, floor) {
                persistable.push(m);
            }
        }
        if persistable.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let mut rows = Vec::with_capacity(persistable.len());
        for m in persistable {
            remove_conflicting_client_msg_rows_tx(
                &mut tx,
                &m.client_msg_id,
                &m.server_id,
                &m.conversation_id,
            )
            .await?;

            let client_msg_id = m.client_msg_id.trim();
            let server_id = m.server_id.trim();
            if !server_id.is_empty() {
                rows.retain(|row: &MessagePersistRow<'_>| {
                    let row_server_id = row.message.server_id.trim();
                    let row_client_msg_id = row.message.client_msg_id.trim();
                    if row_server_id == server_id {
                        return false;
                    }
                    client_msg_id.is_empty()
                        || row_client_msg_id != client_msg_id
                        || row_server_id == server_id
                });
            }
            rows.push(MessagePersistRow::from_message(m));
        }

        let mut latest_per_conversation: HashMap<&str, &IMMessage> = HashMap::new();
        for chunk in rows.chunks(MESSAGE_SAVE_BATCH_INSERT_CHUNK_SIZE) {
            insert_message_rows_tx(&mut tx, chunk).await?;
            upsert_message_fts_rows_tx(&mut tx, chunk).await?;
            replace_reaction_snapshot_rows_tx(&mut tx, chunk).await?;
            for row in chunk {
                let m = row.message;
                let conv_id = m.conversation_id.trim();
                if conv_id.is_empty() {
                    continue;
                }
                match latest_per_conversation.get(conv_id) {
                    Some(prev) if !should_replace_conversation_projection(prev, m) => {}
                    _ => {
                        latest_per_conversation.insert(conv_id, m);
                    }
                }
            }
        }

        for (_, latest) in latest_per_conversation {
            upsert_conversation_snapshot_tx(&mut tx, latest).await?;
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn save_one(&self, message: &IMMessage) -> Result<()> {
        MessageWriter::save_batch(self, std::slice::from_ref(message)).await
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        let recalled = MessageStatus::Recalled as i32;
        if status == recalled {
            sqlx::query(
                r#"UPDATE messages SET status = ?, is_recalled = 1
                   WHERE server_id = ? OR client_msg_id = ?"#,
            )
            .bind(status)
            .bind(message_id)
            .bind(message_id)
            .execute(&self.pool)
            .await
        } else {
            sqlx::query("UPDATE messages SET status = ? WHERE server_id = ? OR client_msg_id = ?")
                .bind(status)
                .bind(message_id)
                .bind(message_id)
                .execute(&self.pool)
                .await
        }
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<bool> {
        let now_ms = now_ms_i64();
        let text = text_for_sqlite_from_content_bytes(&new_content);
        let search_text = search_text_for_content_bytes(&new_content).or_else(|| text.clone());
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let rows = sqlx::query(
            r#"UPDATE messages SET encoded_content = ?, is_edited = 1, text = ?, updated_at = ?
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(&new_content)
        .bind(text.as_deref())
        .bind(now_ms)
        .bind(message_id)
        .bind(message_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?
        .rows_affected();
        if rows > 0 {
            let rows = sqlx::query(
                r#"SELECT server_id, conversation_id
                   FROM messages
                   WHERE server_id = ? OR client_msg_id = ?"#,
            )
            .bind(message_id)
            .bind(message_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            for row in rows {
                let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
                let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
                upsert_message_fts_tx(
                    &mut tx,
                    &server_id,
                    &conversation_id,
                    search_text.as_deref(),
                )
                .await?;
            }
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        if rows == 0 {
            tracing::warn!(
                message_id = %message_id,
                "update_content: no row matched server_id/client_msg_id"
            );
        }
        Ok(rows > 0)
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let deleted_conversation_id = sqlx::query(
            r#"SELECT conversation_id
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?
        .and_then(|row| row.try_get::<String, _>("conversation_id").ok());
        delete_message_fts_by_message_id_tx(&mut tx, message_id).await?;
        sqlx::query(
            r#"DELETE FROM message_reactions
               WHERE message_server_id = ?
                  OR message_server_id IN (
                      SELECT server_id
                      FROM messages
                      WHERE server_id = ? OR client_msg_id = ?
                  )"#,
        )
        .bind(message_id)
        .bind(message_id)
        .bind(message_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM messages WHERE server_id = ? OR client_msg_id = ?")
            .bind(message_id)
            .bind(message_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        if let Some(conversation_id) = deleted_conversation_id {
            refresh_conversation_snapshot_after_message_delete_tx(&mut tx, &conversation_id)
                .await?;
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn rewrite_conversation_id(
        &self,
        from_conversation_id: &str,
        to_conversation_id: &str,
    ) -> Result<u64> {
        let from = from_conversation_id.trim();
        let to = to_conversation_id.trim();
        if from.is_empty() || to.is_empty() || from == to {
            return Ok(0);
        }
        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        let result =
            sqlx::query("UPDATE messages SET conversation_id = ? WHERE conversation_id = ?")
                .bind(to)
                .bind(from)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        sqlx::query("UPDATE messages_fts SET conversation_id = ? WHERE conversation_id = ?")
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        sqlx::query("UPDATE message_reactions SET conversation_id = ? WHERE conversation_id = ?")
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        tx.commit().await.map_err(sqlx_err)?;
        Ok(result.rows_affected())
    }

    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()> {
        let mut message = MessageDeliveryService::sanitize_send_ack_message(message);
        let ack_client_msg_id = client_msg_id.trim();
        if message.client_msg_id.trim().is_empty() && !ack_client_msg_id.is_empty() {
            message.client_msg_id = ack_client_msg_id.to_string();
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        remove_conflicting_client_msg_rows_tx(
            &mut tx,
            ack_client_msg_id,
            &message.server_id,
            &message.conversation_id,
        )
        .await?;
        if message.client_msg_id.trim() != ack_client_msg_id {
            remove_conflicting_client_msg_rows_tx(
                &mut tx,
                &message.client_msg_id,
                &message.server_id,
                &message.conversation_id,
            )
            .await?;
        }
        let extra_json = serde_json::to_string(&message.attributes).unwrap_or_default();
        let mention_users_json = serde_json::to_string(&message.mention_users).unwrap_or_default();
        let extensions_json = extensions_to_json(&message.extensions);
        let text = message_preview_for_storage(&message);
        let search_text = message_search_text_for_storage(&message);
        sqlx::query(
            r#"INSERT OR REPLACE INTO messages (
               server_id, conversation_id, client_msg_id, sender_id, source, conversation_seq, created_at, client_created_at,
               conversation_type, message_type, channel_id, sender_name, sender_avatar,
               sender_display_name, encoded_content, status,
               retention_policy, retention_state,
               is_read, is_recalled, is_edited,
               reply_to, quote_preview, mention_users, mention_all, attributes, extensions, version, updated_at, text,
               sending, failed, is_local, sort_ts)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&message.server_id)
        .bind(&message.conversation_id)
        .bind(&message.client_msg_id)
        .bind(&message.sender_id)
        .bind(message.source)
        .bind(message.conversation_seq as i64)
        .bind(message.created_at as i64)
        .bind(message.client_created_at as i64)
        .bind(message.conversation_type)
        .bind(message.message_type)
        .bind(&message.channel_id)
        .bind(&message.sender_name)
        .bind(&message.sender_avatar)
        .bind(&message.sender_display_name)
        .bind(&message.encoded_content)
        .bind(message.status)
        .bind(encode_optional_proto(&message.retention_policy))
        .bind(encode_optional_proto(&message.retention_state))
        .bind(if message.is_read { 1i32 } else { 0 })
        .bind(if message.is_recalled { 1i32 } else { 0 })
        .bind(if message.is_edited { 1i32 } else { 0 })
        .bind(&message.reply_to)
        .bind(&message.quote_preview)
        .bind(&mention_users_json)
        .bind(if message.mention_all { 1i32 } else { 0 })
        .bind(&extra_json)
        .bind(&extensions_json)
        .bind(message.version as i64)
        .bind(message.updated_at as i64)
        .bind(text.as_deref())
        .bind(if message.local_state.sending { 1i32 } else { 0 })
        .bind(if message.local_state.failed { 1i32 } else { 0 })
        .bind(if message.local_state.is_local { 1i32 } else { 0 })
        .bind(effective_sort_ts_for_persist(&message))
        .execute(&mut *tx)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        upsert_message_fts_tx(
            &mut tx,
            &message.server_id,
            &message.conversation_id,
            search_text.as_deref(),
        )
        .await?;
        replace_reaction_snapshot_tx(&mut tx, &message).await?;
        upsert_conversation_snapshot_tx(&mut tx, &message).await?;
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl MessageStore for SqliteMessageRepo {
    async fn apply_edit_event(
        &self,
        message_id: &str,
        new_content: Vec<u8>,
        edit_version: i32,
    ) -> Result<EditApplyResult> {
        let row = sqlx::query(
            r#"SELECT attributes
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(EditApplyResult::NotFound);
        };

        let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
        let mut attributes = parse_extra(extra_raw.as_deref());
        let current_edit_version = attributes
            .get("currentEditVersion")
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(0);
        if edit_version > 0 && current_edit_version > 0 && edit_version <= current_edit_version {
            return Ok(EditApplyResult::IgnoredStale);
        }

        let now_ms = now_ms_i64();
        let text = text_for_sqlite_from_content_bytes(&new_content);
        let search_text = search_text_for_content_bytes(&new_content).or_else(|| text.clone());
        let next_edit_version = if edit_version > 0 {
            edit_version.max(current_edit_version)
        } else {
            current_edit_version.max(1)
        };
        attributes.insert(
            "currentEditVersion".to_string(),
            next_edit_version.to_string(),
        );
        attributes.insert("messageFsmState".to_string(), "EDITED".to_string());
        attributes.insert("lastEditedAt".to_string(), now_ms.to_string());

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        let rows = sqlx::query(
            r#"UPDATE messages
               SET encoded_content = ?, is_edited = 1, text = ?, updated_at = ?, attributes = ?
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(&new_content)
        .bind(text.as_deref())
        .bind(now_ms)
        .bind(extra_to_json(&attributes))
        .bind(message_id)
        .bind(message_id)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if rows > 0 {
            let rows = sqlx::query(
                r#"SELECT server_id, conversation_id
                   FROM messages
                   WHERE server_id = ? OR client_msg_id = ?"#,
            )
            .bind(message_id)
            .bind(message_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(sqlx_err)?;
            for row in rows {
                let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
                let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_err)?;
                upsert_message_fts_tx(
                    &mut tx,
                    &server_id,
                    &conversation_id,
                    search_text.as_deref(),
                )
                .await?;
            }
        }
        tx.commit().await.map_err(sqlx_err)?;

        Ok(if rows > 0 {
            EditApplyResult::Applied
        } else {
            EditApplyResult::NotFound
        })
    }

    async fn mark_outgoing_read_upto_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        read_seq: u64,
    ) -> Result<()> {
        if conversation_id.trim().is_empty() || sender_user_id.trim().is_empty() || read_seq == 0 {
            return Ok(());
        }
        let created = MessageStatus::Created as i32;
        let sent = MessageStatus::Sent as i32;
        let persisted = MessageStatus::Persisted as i32;
        sqlx::query(
            r#"UPDATE messages
               SET status = CASE WHEN status = ? THEN ? ELSE status END,
                   is_read = 1
               WHERE conversation_id = ?
                 AND sender_id = ?
                 AND conversation_seq > 0
                 AND conversation_seq <= ?
                 AND status IN (?, ?, ?)"#,
        )
        .bind(created)
        .bind(sent)
        .bind(conversation_id)
        .bind(sender_user_id)
        .bind(read_seq as i64)
        .bind(created)
        .bind(sent)
        .bind(persisted)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn reconcile_outgoing_read_by_peer_seq(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        peer_read_seq: u64,
    ) -> Result<()> {
        if conversation_id.trim().is_empty() || sender_user_id.trim().is_empty() {
            return Ok(());
        }
        if peer_read_seq > 0 {
            self.mark_outgoing_read_upto_seq(conversation_id, sender_user_id, peer_read_seq)
                .await?;
        }
        sqlx::query(
            r#"UPDATE messages
               SET is_read = 0
               WHERE conversation_id = ?
                 AND sender_id = ?
                 AND conversation_seq > ?
                 AND is_read = 1"#,
        )
        .bind(conversation_id)
        .bind(sender_user_id)
        .bind(peer_read_seq as i64)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        Ok(())
    }

    async fn apply_reaction(
        &self,
        conversation_id: &str,
        message_server_id: &str,
        user_id: &str,
        emoji: &str,
        action: i32,
    ) -> Result<()> {
        if message_server_id.is_empty() || user_id.is_empty() || emoji.is_empty() {
            return Ok(());
        }
        let add_action = ReactionAction::Add as i32;
        let remove_action = ReactionAction::Remove as i32;
        if action != add_action && action != remove_action {
            return Ok(());
        }
        let now = now_ms_i64();
        if action == remove_action {
            sqlx::query(
                r#"DELETE FROM message_reactions
                   WHERE message_server_id = ? AND emoji = ? AND user_id = ?"#,
            )
            .bind(message_server_id)
            .bind(emoji)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
            debug!(
                conversation_id = %conversation_id,
                message_server_id = %message_server_id,
                user_id = %user_id,
                emoji = %emoji,
                action = action,
                "apply_reaction remove"
            );
            return Ok(());
        }
        sqlx::query(
            r#"INSERT OR REPLACE INTO message_reactions
               (message_server_id, conversation_id, emoji, user_id, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(message_server_id)
        .bind(conversation_id)
        .bind(emoji)
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        debug!(
            conversation_id = %conversation_id,
            message_server_id = %message_server_id,
            user_id = %user_id,
            emoji = %emoji,
            action = action,
            "apply_reaction add"
        );
        refresh_reactions_json_snapshot(&self.pool, message_server_id).await?;
        Ok(())
    }

    async fn apply_reaction_event(
        &self,
        conversation_id: &str,
        message_server_id: &str,
        user_id: &str,
        emoji: &str,
        action: i32,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        if message_server_id.is_empty() || user_id.is_empty() || emoji.is_empty() {
            return Ok(OperationApplyResult::NotFound);
        }
        let add_action = ReactionAction::Add as i32;
        let remove_action = ReactionAction::Remove as i32;
        if action != add_action && action != remove_action {
            return Ok(OperationApplyResult::NotFound);
        }

        let seq_key = format!("lastReactionEventSeq:{user_id}:{emoji}");
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_server_id,
            &seq_key,
            event_seq,
            |_| {},
        )
        .await?;
        if applied == OperationApplyResult::IgnoredStale {
            return Ok(OperationApplyResult::IgnoredStale);
        }

        let now = now_ms_i64();
        if action == remove_action {
            sqlx::query(
                r#"DELETE FROM message_reactions
                   WHERE message_server_id = ? AND emoji = ? AND user_id = ?"#,
            )
            .bind(message_server_id)
            .bind(emoji)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?;
            debug!(
                conversation_id = %conversation_id,
                message_server_id = %message_server_id,
                user_id = %user_id,
                emoji = %emoji,
                action = action,
                "apply_reaction_event remove"
            );
            refresh_reactions_json_snapshot(&self.pool, message_server_id).await?;
            return Ok(OperationApplyResult::Applied);
        }
        sqlx::query(
            r#"INSERT OR REPLACE INTO message_reactions
               (message_server_id, conversation_id, emoji, user_id, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(message_server_id)
        .bind(conversation_id)
        .bind(emoji)
        .bind(user_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?;
        debug!(
            conversation_id = %conversation_id,
            message_server_id = %message_server_id,
            user_id = %user_id,
            emoji = %emoji,
            action = action,
            "apply_reaction_event add"
        );
        refresh_reactions_json_snapshot(&self.pool, message_server_id).await?;
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_delete_event(
        &self,
        message_id: &str,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let row = sqlx::query(
            r#"SELECT server_id, attributes
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(OperationApplyResult::NotFound);
        };

        let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
        let attributes = parse_extra(extra_raw.as_deref());
        if is_stale_operation(&attributes, "lastDeleteEventSeq", event_seq) {
            return Ok(OperationApplyResult::IgnoredStale);
        }

        self.delete(message_id).await?;
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_pin_event(
        &self,
        message_id: &str,
        enabled: bool,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastPinEventSeq",
            event_seq,
            |attributes| {
                attributes.insert(
                    "pinned".to_string(),
                    if enabled { "true" } else { "false" }.to_string(),
                );
            },
        )
        .await
    }

    async fn apply_mark_event(
        &self,
        message_id: &str,
        mark_type: i32,
        color: Option<&str>,
        set_mark: bool,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let seq_key = format!("lastMarkEventSeq:{mark_type}");
        apply_message_extra_with_seq(&self.pool, message_id, &seq_key, event_seq, |attributes| {
            if set_mark {
                attributes.insert("markType".to_string(), mark_type.to_string());
                if let Some(c) = color {
                    if !c.trim().is_empty() {
                        attributes.insert("markColor".to_string(), c.trim().to_string());
                    } else {
                        attributes.remove("markColor");
                    }
                } else {
                    attributes.remove("markColor");
                }
            } else {
                attributes.remove("markType");
                attributes.remove("markColor");
            }
        })
        .await
    }

    async fn apply_retention_scheduled_event(
        &self,
        message_id: &str,
        policy: &MessageRetentionPolicy,
        state: &MessageRetentionState,
        scheduled_at: i64,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastRetentionScheduledEventSeq",
            event_seq,
            |attributes| {
                attributes.insert("retention_event".to_string(), "scheduled".to_string());
            },
        )
        .await?;
        if !matches!(applied, OperationApplyResult::Applied) {
            return Ok(applied);
        }
        let rows = sqlx::query(
            r#"UPDATE messages
               SET retention_policy = ?,
                   retention_state = ?,
                   updated_at = ?
               WHERE (server_id = ? OR client_msg_id = ?)"#,
        )
        .bind(policy.encode_to_vec())
        .bind(state.encode_to_vec())
        .bind(scheduled_at)
        .bind(message_id)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Ok(OperationApplyResult::NotFound);
        }
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_retention_expired_event(
        &self,
        message_id: &str,
        state: &MessageRetentionState,
        expired_at: i64,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastRetentionExpiredEventSeq",
            event_seq,
            |attributes| {
                attributes.insert("retention_event".to_string(), "expired".to_string());
                attributes.insert(
                    "retention_placeholder".to_string(),
                    "该消息已过期".to_string(),
                );
            },
        )
        .await?;
        if !matches!(applied, OperationApplyResult::Applied) {
            return Ok(applied);
        }
        let now_ms = now_ms_i64();
        let rows = if retention_hides_content(state) {
            sqlx::query(
                r#"UPDATE messages
                   SET retention_state = ?,
                       encoded_content = ?,
                       text = NULL,
                       updated_at = ?
                   WHERE (server_id = ? OR client_msg_id = ?)"#,
            )
            .bind(state.encode_to_vec())
            .bind(Vec::<u8>::new())
            .bind(now_ms.max(expired_at))
            .bind(message_id)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected()
        } else {
            sqlx::query(
                r#"UPDATE messages
                   SET retention_state = ?,
                       updated_at = ?
                   WHERE (server_id = ? OR client_msg_id = ?)"#,
            )
            .bind(state.encode_to_vec())
            .bind(now_ms.max(expired_at))
            .bind(message_id)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(sqlx_err)?
            .rows_affected()
        };
        if rows == 0 {
            return Ok(OperationApplyResult::NotFound);
        }
        Ok(OperationApplyResult::Applied)
    }

    async fn apply_retention_purged_event(
        &self,
        message_id: &str,
        state: &MessageRetentionState,
        purged_at: i64,
        event_seq: Option<u64>,
    ) -> Result<OperationApplyResult> {
        let applied = apply_message_extra_with_seq(
            &self.pool,
            message_id,
            "lastRetentionPurgedEventSeq",
            event_seq,
            |attributes| {
                attributes.insert("retention_event".to_string(), "purged".to_string());
                attributes.insert(
                    "retention_placeholder".to_string(),
                    "该消息已清理".to_string(),
                );
            },
        )
        .await?;
        if !matches!(applied, OperationApplyResult::Applied) {
            return Ok(applied);
        }
        let now_ms = now_ms_i64();
        let rows = sqlx::query(
            r#"UPDATE messages
               SET retention_state = ?,
                   encoded_content = ?,
                   text = NULL,
                   updated_at = ?
               WHERE (server_id = ? OR client_msg_id = ?)"#,
        )
        .bind(state.encode_to_vec())
        .bind(Vec::<u8>::new())
        .bind(now_ms.max(purged_at))
        .bind(message_id)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(sqlx_err)?
        .rows_affected();
        if rows == 0 {
            return Ok(OperationApplyResult::NotFound);
        }
        Ok(OperationApplyResult::Applied)
    }

    async fn list_reactions(
        &self,
        message_server_ids: &[String],
    ) -> Result<HashMap<String, Vec<ReactionEntry>>> {
        if message_server_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut id_keys: Vec<String> = message_server_ids
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if id_keys.is_empty() {
            return Ok(HashMap::new());
        }
        id_keys.sort();
        id_keys.dedup();

        let mut alias_qb = QueryBuilder::<Sqlite>::new(
            "SELECT server_id, client_msg_id FROM messages WHERE server_id IN (",
        );
        let mut alias_sep_a = alias_qb.separated(", ");
        for id in &id_keys {
            alias_sep_a.push_bind(id);
        }
        alias_qb.push(") OR client_msg_id IN (");
        let mut alias_sep_b = alias_qb.separated(", ");
        for id in &id_keys {
            alias_sep_b.push_bind(id);
        }
        alias_qb.push(")");
        let alias_rows = alias_qb
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;

        let mut canonical: HashMap<String, String> = HashMap::new();
        for row in alias_rows {
            let sid: String = row.try_get("server_id").map_err(sqlx_err)?;
            let cid: String = row.try_get("client_msg_id").map_err(sqlx_err)?;
            let sid_t = sid.trim();
            let cid_t = cid.trim();
            if !sid_t.is_empty() {
                canonical.insert(sid_t.to_string(), sid_t.to_string());
            }
            if !cid_t.is_empty() {
                canonical.insert(cid_t.to_string(), sid_t.to_string());
            }
        }

        let mut qb = QueryBuilder::<Sqlite>::new(
            "SELECT message_server_id, emoji, user_id FROM message_reactions WHERE message_server_id IN (",
        );
        let mut separated = qb.separated(", ");
        for id in &id_keys {
            separated.push_bind(id);
        }
        qb.push(
            ") OR message_server_id IN (SELECT client_msg_id FROM messages WHERE server_id IN (",
        );
        let mut sid_sep = qb.separated(", ");
        for id in &id_keys {
            sid_sep.push_bind(id);
        }
        qb.push(")) ORDER BY updated_at ASC");
        let rows = qb.build().fetch_all(&self.pool).await.map_err(sqlx_err)?;

        let mut grouped: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
        for row in rows {
            let msg_id: String = row.try_get("message_server_id").map_err(sqlx_err)?;
            let emoji: String = row.try_get("emoji").map_err(sqlx_err)?;
            let user_id: String = row.try_get("user_id").map_err(sqlx_err)?;
            let resolved_id = canonical
                .get(msg_id.trim())
                .cloned()
                .unwrap_or_else(|| msg_id.trim().to_string());
            if resolved_id.is_empty() {
                continue;
            }
            grouped
                .entry(resolved_id)
                .or_default()
                .entry(emoji)
                .or_default()
                .push(user_id);
        }

        let mut out: HashMap<String, Vec<ReactionEntry>> = HashMap::new();
        for (msg_id, emoji_map) in grouped {
            let mut reactions = Vec::with_capacity(emoji_map.len());
            for (emoji, user_ids) in emoji_map {
                reactions.push(ReactionEntry {
                    emoji,
                    count: user_ids.len() as u32,
                    user_ids,
                });
            }
            out.insert(msg_id, reactions);
        }
        Ok(out)
    }

    async fn set_message_flag(
        &self,
        message_id: &str,
        flag_key: &str,
        enabled: bool,
    ) -> Result<()> {
        if message_id.trim().is_empty() || flag_key.trim().is_empty() {
            return Ok(());
        }
        let rows = sqlx::query(
            r#"SELECT server_id, attributes FROM messages
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        for row in rows {
            let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
            let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
            let mut attributes = parse_extra(extra_raw.as_deref());
            attributes.insert(
                flag_key.to_string(),
                if enabled { "true" } else { "false" }.to_string(),
            );
            sqlx::query("UPDATE messages SET attributes = ? WHERE server_id = ?")
                .bind(extra_to_json(&attributes))
                .bind(server_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn set_message_mark(
        &self,
        message_id: &str,
        mark_type: i32,
        color: Option<&str>,
    ) -> Result<()> {
        let rows = sqlx::query(
            r#"SELECT server_id, attributes FROM messages
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        for row in rows {
            let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
            let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
            let mut attributes = parse_extra(extra_raw.as_deref());
            attributes.insert("markType".to_string(), mark_type.to_string());
            if let Some(c) = color {
                if !c.trim().is_empty() {
                    attributes.insert("markColor".to_string(), c.trim().to_string());
                } else {
                    attributes.remove("markColor");
                }
            } else {
                attributes.remove("markColor");
            }
            sqlx::query("UPDATE messages SET attributes = ? WHERE server_id = ?")
                .bind(extra_to_json(&attributes))
                .bind(server_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn clear_message_mark(&self, message_id: &str, _mark_type: i32) -> Result<()> {
        let rows = sqlx::query(
            r#"SELECT server_id, attributes FROM messages
               WHERE server_id = ? OR client_msg_id = ?"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .map_err(sqlx_err)?;

        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        for row in rows {
            let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
            let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
            let mut attributes = parse_extra(extra_raw.as_deref());
            attributes.remove("markType");
            attributes.remove("markColor");
            sqlx::query("UPDATE messages SET attributes = ? WHERE server_id = ?")
                .bind(extra_to_json(&attributes))
                .bind(server_id)
                .execute(&mut *tx)
                .await
                .map_err(sqlx_err)?;
        }
        tx.commit().await.map_err(sqlx_err)?;
        Ok(())
    }

    async fn heal_orphan_sending_messages(
        &self,
        sender_user_id: &str,
        pending_client_msg_ids: &[String],
    ) -> Result<Vec<String>> {
        if sender_user_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut select_qb = QueryBuilder::<Sqlite>::new(
            "SELECT client_msg_id FROM messages WHERE sending = 1 AND failed = 0 AND is_local = 1 AND sender_id = ",
        );
        select_qb.push_bind(sender_user_id);
        if !pending_client_msg_ids.is_empty() {
            select_qb.push(" AND client_msg_id NOT IN (");
            let mut separated = select_qb.separated(", ");
            for id in pending_client_msg_ids {
                separated.push_bind(id);
            }
            select_qb.push(")");
        }
        let orphan_rows = select_qb
            .build_query_as::<(String,)>()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if orphan_rows.is_empty() {
            return Ok(Vec::new());
        }

        let orphan_client_ids: Vec<String> = orphan_rows.into_iter().map(|(id,)| id).collect();
        let mut update_qb =
            QueryBuilder::<Sqlite>::new("UPDATE messages SET sending = 0, failed = 1, status = ");
        update_qb.push_bind(MessageStatus::Failed as i32);
        update_qb.push(", updated_at = ");
        update_qb.push_bind(now_ms_i64());
        update_qb.push(" WHERE client_msg_id IN (");
        let mut separated = update_qb.separated(", ");
        for id in &orphan_client_ids {
            separated.push_bind(id);
        }
        update_qb.push(")");
        let query = update_qb.build();
        query.execute(&self.pool).await.map_err(sqlx_err)?;
        Ok(orphan_client_ids)
    }

    async fn heal_cross_account_pending_messages(
        &self,
        sender_user_id: &str,
        pending_client_msg_ids: &[String],
    ) -> Result<Vec<String>> {
        if sender_user_id.trim().is_empty() || pending_client_msg_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut select_qb = QueryBuilder::<Sqlite>::new(
            "SELECT client_msg_id FROM messages WHERE client_msg_id IN (",
        );
        {
            let mut separated = select_qb.separated(", ");
            for id in pending_client_msg_ids {
                separated.push_bind(id);
            }
        }
        select_qb.push(") AND (sender_id = '' OR sender_id != ");
        select_qb.push_bind(sender_user_id);
        select_qb.push(")");
        let mismatched_rows = select_qb
            .build_query_as::<(String,)>()
            .fetch_all(&self.pool)
            .await
            .map_err(sqlx_err)?;
        if mismatched_rows.is_empty() {
            return Ok(Vec::new());
        }
        let mismatched_client_ids: Vec<String> =
            mismatched_rows.into_iter().map(|(id,)| id).collect();

        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        let mut update_qb =
            QueryBuilder::<Sqlite>::new("UPDATE messages SET sending = 0, failed = 1, status = ");
        update_qb.push_bind(MessageStatus::Failed as i32);
        update_qb.push(", updated_at = ");
        update_qb.push_bind(now_ms_i64());
        update_qb.push(" WHERE client_msg_id IN (");
        {
            let mut separated = update_qb.separated(", ");
            for id in &mismatched_client_ids {
                separated.push_bind(id);
            }
        }
        update_qb.push(")");
        update_qb
            .build()
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        let mut delete_qb =
            QueryBuilder::<Sqlite>::new("DELETE FROM pending_sends WHERE client_msg_id IN (");
        {
            let mut separated = delete_qb.separated(", ");
            for id in &mismatched_client_ids {
                separated.push_bind(id);
            }
        }
        delete_qb.push(")");
        delete_qb
            .build()
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;

        tx.commit().await.map_err(sqlx_err)?;
        Ok(mismatched_client_ids)
    }
}

fn is_stale_operation(
    attributes: &HashMap<String, String>,
    seq_key: &str,
    incoming_event_seq: Option<u64>,
) -> bool {
    let Some(incoming_event_seq) =
        incoming_event_seq.filter(|conversation_seq| *conversation_seq > 0)
    else {
        return false;
    };
    let current_event_seq = attributes
        .get(seq_key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    current_event_seq > 0 && incoming_event_seq <= current_event_seq
}

async fn apply_message_extra_with_seq<F>(
    pool: &SqlitePool,
    message_id: &str,
    seq_key: &str,
    incoming_event_seq: Option<u64>,
    mut apply: F,
) -> Result<OperationApplyResult>
where
    F: FnMut(&mut HashMap<String, String>),
{
    let rows = sqlx::query(
        r#"SELECT server_id, attributes FROM messages
           WHERE server_id = ? OR client_msg_id = ?"#,
    )
    .bind(message_id)
    .bind(message_id)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    if rows.is_empty() {
        return Ok(OperationApplyResult::NotFound);
    }

    let mut tx = pool.begin().await.map_err(sqlx_err)?;
    let mut applied_any = false;
    for row in rows {
        let server_id: String = row.try_get("server_id").map_err(sqlx_err)?;
        let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
        let mut attributes = parse_extra(extra_raw.as_deref());
        if is_stale_operation(&attributes, seq_key, incoming_event_seq) {
            continue;
        }
        if let Some(conversation_seq) =
            incoming_event_seq.filter(|conversation_seq| *conversation_seq > 0)
        {
            attributes.insert(seq_key.to_string(), conversation_seq.to_string());
        }
        apply(&mut attributes);
        sqlx::query("UPDATE messages SET attributes = ? WHERE server_id = ?")
            .bind(extra_to_json(&attributes))
            .bind(server_id)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_err)?;
        applied_any = true;
    }
    tx.commit().await.map_err(sqlx_err)?;

    Ok(if applied_any {
        OperationApplyResult::Applied
    } else {
        OperationApplyResult::IgnoredStale
    })
}

#[cfg(test)]
mod tests {
    use super::SqliteMessageRepo;
    use crate::domain::{
        ConversationReader, ConversationWriter, EditApplyResult, MessageReader, MessageStore,
        MessageWriter, OperationApplyResult,
    };
    use crate::infrastructure::persistence::sqlite::conversation_repo::SqliteConversationRepo;
    use crate::infrastructure::persistence::sqlite_init_schema;
    use crate::model::conversation::ConversationType;
    use crate::model::message::{MessageStatus, ReactionAction};
    use crate::model::{
        Conversation, IMMessage, MessageSearchKind, MessageSearchQuery, MessageType,
    };
    use sqlx::SqlitePool;

    async fn make_repo() -> SqliteMessageRepo {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        SqliteMessageRepo::new(pool)
    }

    fn text_message(
        server_id: &str,
        conversation_id: &str,
        sender_id: &str,
        conversation_seq: u64,
        created_at: u64,
        text: &str,
    ) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = server_id.to_string();
        message.client_msg_id = format!("client-{server_id}");
        message.conversation_id = conversation_id.to_string();
        message.sender_id = sender_id.to_string();
        message.conversation_seq = conversation_seq;
        message.created_at = created_at;
        message.client_created_at = created_at;
        message.content = Some(crate::model::Elem::Text(
            crate::content::message_elem::TextElem {
                text: text.to_string(),
                mentions: Vec::new(),
            },
        ));
        message.materialize_encoded_content_from_elem();
        message
    }

    #[tokio::test]
    async fn get_by_conversation_repairs_single_chat_channel_alias_messages() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        let message_repo = SqliteMessageRepo::new(pool.clone());
        let conversation_repo = SqliteConversationRepo::new(pool.clone());

        let mut conversation = Conversation::from_conversation_id("cid-canonical".to_string());
        conversation.conversation_type = ConversationType::Single;
        conversation.channel_id = "peer-12".to_string();
        conversation.display_name = "peer-12".to_string();
        conversation.unread_count = 17;
        conversation.max_seq = 17;
        conversation_repo.save_one(&conversation).await.unwrap();

        let mut message = text_message(
            "server-alias-window",
            "peer-12",
            "peer-12",
            17,
            17_000,
            "hello-from-peer",
        );
        message.conversation_type = ConversationType::Single.to_proto_int();
        message.channel_id = "peer-12".to_string();
        message_repo.save_one(&message).await.unwrap();

        let messages = message_repo
            .get_by_conversation("cid-canonical", 0, 20)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].conversation_id, "cid-canonical");
        assert_eq!(messages[0].server_id, "server-alias-window");

        assert!(
            message_repo
                .get_by_conversation("peer-12", 0, 20)
                .await
                .unwrap()
                .is_empty()
        );
    }

    fn file_message(
        server_id: &str,
        conversation_id: &str,
        sender_id: &str,
        conversation_seq: u64,
        created_at: u64,
        file_name: &str,
        mime_type: &str,
        description: &str,
    ) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = server_id.to_string();
        message.client_msg_id = format!("client-{server_id}");
        message.conversation_id = conversation_id.to_string();
        message.sender_id = sender_id.to_string();
        message.conversation_seq = conversation_seq;
        message.created_at = created_at;
        message.client_created_at = created_at;
        message.message_type = MessageType::File as i32;
        message.content = Some(crate::model::Elem::File(
            crate::content::message_elem::FileElem {
                file_id: format!("file-{server_id}"),
                file_name: file_name.to_string(),
                mime_type: mime_type.to_string(),
                file_size: 1024,
                url: String::new(),
                description: description.to_string(),
            },
        ));
        message.materialize_encoded_content_from_elem();
        message
    }

    #[tokio::test]
    async fn conversation_projection_uses_seq_over_timestamp_for_server_messages() {
        let repo = make_repo().await;
        let older_seq_future_time = text_message(
            "server-10",
            "conv-order",
            "u2",
            10,
            2_000,
            "older conversation_seq",
        );
        let newer_seq_past_time = text_message(
            "server-11",
            "conv-order",
            "u2",
            11,
            1_000,
            "newer conversation_seq",
        );

        repo.save_batch(&[older_seq_future_time]).await.unwrap();
        repo.save_batch(&[newer_seq_past_time]).await.unwrap();

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-order")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(conversation.max_seq, 11);
        assert_eq!(conversation.last_message_id.as_deref(), Some("server-11"));
        assert_eq!(conversation.last_sender_id.as_deref(), Some("u2"));
    }

    #[tokio::test]
    async fn deleting_last_message_rebuilds_conversation_preview_from_previous_visible_message() {
        let repo = make_repo().await;
        let first = text_message("server-del-1", "conv-delete", "u2", 1, 1_000, "first");
        let second = text_message("server-del-2", "conv-delete", "u2", 2, 2_000, "second");
        repo.save_batch(&[first, second]).await.unwrap();

        repo.delete("server-del-2").await.unwrap();

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-delete")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(conversation.max_seq, 2);
        assert_eq!(
            conversation.last_message_id.as_deref(),
            Some("server-del-1")
        );
        assert_eq!(conversation.last_message_at, Some(1_000));
    }

    #[tokio::test]
    async fn pending_local_message_updates_conversation_preview_with_client_time() {
        let repo = make_repo().await;
        let server = text_message(
            "server-pending-base",
            "conv-pending",
            "u2",
            7,
            1_000,
            "server",
        );
        let mut pending = text_message("", "conv-pending", "u1", 0, 0, "pending");
        pending.client_msg_id = "client-pending-new".to_string();
        pending.client_created_at = 5_000;
        pending.local_state.sending = true;
        pending.local_state.is_local = true;

        repo.save_batch(&[server]).await.unwrap();
        repo.save_batch(&[pending]).await.unwrap();

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-pending")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(
            conversation.last_message_id.as_deref(),
            Some("client-pending-new")
        );
        assert!(conversation.last_message_at.unwrap_or_default() >= 5_000);
        assert!(
            conversation
                .last_message_preview
                .as_deref()
                .unwrap_or_default()
                .contains("pending")
        );
    }

    #[tokio::test]
    async fn advanced_search_filters_conversation_sender_time_and_media_kind() {
        let repo = make_repo().await;
        let text = text_message("server-search-text", "conv-search", "u2", 1, 1_000, "alpha");
        let mut image = text_message(
            "server-search-image",
            "conv-search",
            "u3",
            2,
            2_000,
            "alpha image",
        );
        image.message_type = MessageType::Image as i32;
        let mut other_sender = text_message(
            "server-search-other",
            "conv-search",
            "u4",
            3,
            3_000,
            "alpha other",
        );
        other_sender.message_type = MessageType::Image as i32;
        let other_conversation = text_message(
            "server-search-foreign",
            "conv-foreign",
            "u3",
            4,
            4_000,
            "alpha image",
        );
        repo.save_batch(&[text, image, other_sender, other_conversation])
            .await
            .unwrap();

        let results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("alpha".to_string()),
                conversation_id: Some("conv-search".to_string()),
                sender_id: Some("u3".to_string()),
                from_time: Some(1_500),
                to_time: Some(2_500),
                kinds: vec![MessageSearchKind::Media],
                limit: 20,
                include_recalled: false,
            })
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].server_id, "server-search-image");

        let wildcard_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("%".to_string()),
                limit: 20,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert!(wildcard_results.is_empty());
    }

    #[tokio::test]
    async fn file_search_matches_typed_file_fields() {
        let repo = make_repo().await;
        let file = file_message(
            "server-file-search",
            "conv-file-search",
            "u2",
            1,
            1_000,
            "合同终稿.pdf",
            "application/pdf",
            "Q2 procurement contract",
        );
        let text = text_message(
            "server-file-search-text",
            "conv-file-search",
            "u2",
            2,
            2_000,
            "合同终稿",
        );
        repo.save_batch(&[file, text]).await.unwrap();

        let file_name_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("合同".to_string()),
                kinds: vec![MessageSearchKind::File],
                limit: 10,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(file_name_results.len(), 1);
        assert_eq!(file_name_results[0].server_id, "server-file-search");

        let mime_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("application/pdf".to_string()),
                kinds: vec![MessageSearchKind::File],
                limit: 10,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(mime_results.len(), 1);
        assert_eq!(mime_results[0].server_id, "server-file-search");

        let media_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("procurement".to_string()),
                kinds: vec![MessageSearchKind::Media],
                limit: 10,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(media_results.len(), 1);
        assert_eq!(media_results[0].server_id, "server-file-search");
    }

    #[tokio::test]
    async fn file_search_index_tracks_content_update() {
        let repo = make_repo().await;
        let original = file_message(
            "server-file-update",
            "conv-file-update",
            "u2",
            1,
            1_000,
            "old-plan.pdf",
            "application/pdf",
            "legacy attachment",
        );
        repo.save_batch(&[original]).await.unwrap();

        let updated = file_message(
            "server-file-update",
            "conv-file-update",
            "u2",
            1,
            2_000,
            "new-roadmap.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "launch budget sheet",
        );
        assert!(
            repo.update_content("server-file-update", updated.encoded_content)
                .await
                .unwrap()
        );

        let old_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("old-plan".to_string()),
                kinds: vec![MessageSearchKind::File],
                limit: 10,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert!(old_results.is_empty());

        let new_results = repo
            .search_by_query(&MessageSearchQuery {
                keyword: Some("roadmap".to_string()),
                kinds: vec![MessageSearchKind::File],
                limit: 10,
                ..MessageSearchQuery::default()
            })
            .await
            .unwrap();
        assert_eq!(new_results.len(), 1);
        assert_eq!(new_results[0].server_id, "server-file-update");
    }

    #[tokio::test]
    async fn short_keyword_search_uses_content_fallback_without_metadata_matches() {
        let repo = make_repo().await;
        let content = text_message(
            "server-short-content",
            "conv-short",
            "u2",
            1,
            1_000,
            "生产事故复盘",
        );
        let mut metadata_only = text_message(
            "server-short-metadata",
            "conv-short",
            "u3",
            2,
            2_000,
            "普通通知",
        );
        metadata_only
            .attributes
            .insert("debugNote".to_string(), "事故".to_string());
        repo.save_batch(&[content, metadata_only]).await.unwrap();

        let results = repo.search("事故", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].server_id, "server-short-content");

        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, conversation_seq,
                   created_at, client_created_at, conversation_type, message_type,
                   channel_id, encoded_content, text, attributes, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', NULL, ?, ?, ?)"#,
        )
        .bind("server-short-content-text")
        .bind("conv-short")
        .bind("client-short-content-text")
        .bind("u4")
        .bind(3_i64)
        .bind(3_000_i64)
        .bind(3_000_i64)
        .bind(1_i32)
        .bind(MessageType::Text as i32)
        .bind("u4")
        .bind(r#"{"contentText":"短词内容","debugNote":"ignored"}"#)
        .bind(3_000_i64)
        .bind(3_000_i64)
        .execute(&repo.pool)
        .await
        .unwrap();

        let content_text_results = repo.search("短词", 10).await.unwrap();
        assert_eq!(content_text_results.len(), 1);
        assert_eq!(
            content_text_results[0].server_id,
            "server-short-content-text"
        );
    }

    #[tokio::test]
    async fn init_schema_backfills_fts_for_existing_messages() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlite_init_schema(&pool).await.unwrap();
        sqlx::query(
            r#"INSERT INTO messages (
                   server_id, conversation_id, client_msg_id, sender_id, conversation_seq,
                   created_at, client_created_at, conversation_type, message_type,
                   channel_id, encoded_content, text, sort_ts, updated_at
               ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, X'', ?, ?, ?)"#,
        )
        .bind("server-fts-legacy")
        .bind("conv-fts")
        .bind("client-fts-legacy")
        .bind("u2")
        .bind(1_i64)
        .bind(1_000_i64)
        .bind(1_000_i64)
        .bind(1_i32)
        .bind(MessageType::Text as i32)
        .bind("u2")
        .bind("legacy searchable payload")
        .bind(1_000_i64)
        .bind(1_000_i64)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM messages_fts")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA user_version = 0")
            .execute(&pool)
            .await
            .unwrap();

        sqlite_init_schema(&pool).await.unwrap();
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version, 2);

        let repo = SqliteMessageRepo::new(pool);
        let results = repo.search("searchable", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].server_id, "server-fts-legacy");
    }

    #[tokio::test]
    async fn fts_search_tracks_save_update_and_delete() {
        let repo = make_repo().await;
        let message = text_message(
            "server-fts-live",
            "conv-fts-live",
            "u2",
            1,
            1_000,
            "生产事故复盘",
        );
        repo.save_batch(&[message]).await.unwrap();

        let saved = repo.search("生产事故", 10).await.unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].server_id, "server-fts-live");

        let updated = text_message(
            "server-fts-live",
            "conv-fts-live",
            "u2",
            1,
            2_000,
            "索引更新完成",
        );
        assert!(
            repo.update_content("server-fts-live", updated.encoded_content)
                .await
                .unwrap()
        );

        assert!(repo.search("生产事故", 10).await.unwrap().is_empty());
        let after_update = repo.search("索引更新", 10).await.unwrap();
        assert_eq!(after_update.len(), 1);
        assert_eq!(after_update[0].server_id, "server-fts-live");

        repo.delete("server-fts-live").await.unwrap();
        assert!(repo.search("索引更新", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_client_only_pending_message_rebuilds_preview() {
        let repo = make_repo().await;
        let server = text_message(
            "server-client-delete",
            "conv-client-delete",
            "u2",
            3,
            1_000,
            "server",
        );
        let mut pending = text_message("", "conv-client-delete", "u1", 0, 2_000, "pending");
        pending.client_msg_id = "client-only-delete".to_string();
        pending.local_state.sending = true;
        pending.local_state.is_local = true;

        repo.save_batch(&[server, pending]).await.unwrap();
        repo.delete("client-only-delete").await.unwrap();

        assert!(repo.get("client-only-delete").await.unwrap().is_none());

        let conversations = SqliteConversationRepo::new(repo.pool.clone());
        let conversation = conversations
            .get("conv-client-delete")
            .await
            .unwrap()
            .expect("conversation snapshot");

        assert_eq!(
            conversation.last_message_id.as_deref(),
            Some("server-client-delete")
        );
        assert!(
            conversation
                .last_message_preview
                .as_deref()
                .unwrap_or_default()
                .contains("server")
        );
    }

    #[tokio::test]
    async fn update_after_ack_clamps_self_echo_to_sent() {
        let repo = make_repo().await;
        let mut pending = IMMessage::new(flare_proto::common::Message::default());
        pending.server_id = "client-ack-1".to_string();
        pending.client_msg_id = "client-ack-1".to_string();
        pending.conversation_id = "conv-ack".to_string();
        pending.sender_id = "u1".to_string();
        pending.status = MessageStatus::Created as i32;
        pending.local_state.sending = true;
        pending.local_state.is_local = true;
        repo.save_batch(&[pending.clone()]).await.unwrap();

        let mut echoed = pending;
        echoed.server_id = "server-ack-1".to_string();
        echoed.conversation_seq = 11;
        echoed.status = MessageStatus::Persisted as i32;
        echoed.is_read = true;

        repo.update_after_ack("client-ack-1", &echoed)
            .await
            .unwrap();

        let stored = repo.get("server-ack-1").await.unwrap().unwrap();
        assert_eq!(stored.status, MessageStatus::Sent as i32);
        assert!(!stored.is_read);
        assert!(!stored.local_state.sending);
        assert!(!stored.local_state.is_local);
        assert!(repo.get("client-ack-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_batch_collapses_self_echo_with_same_client_msg_id() {
        let repo = make_repo().await;
        let mut pending = IMMessage::new(flare_proto::common::Message::default());
        pending.server_id = "client-dupe-1".to_string();
        pending.client_msg_id = "client-dupe-1".to_string();
        pending.conversation_id = "conv-dupe".to_string();
        pending.sender_id = "u1".to_string();
        pending.status = MessageStatus::Created as i32;
        pending.local_state.sending = true;
        pending.local_state.is_local = true;
        repo.save_batch(&[pending.clone()]).await.unwrap();

        let mut echoed = pending;
        echoed.server_id = "server-dupe-1".to_string();
        echoed.conversation_seq = 12;
        echoed.created_at = 2_000;
        echoed.status = MessageStatus::Persisted as i32;
        echoed.local_state.sending = false;
        echoed.local_state.is_local = false;
        repo.save_batch(&[echoed]).await.unwrap();

        assert!(repo.get("client-dupe-1").await.unwrap().is_none());

        let by_client = repo
            .get_by_client_msg_id("client-dupe-1")
            .await
            .unwrap()
            .expect("canonical message");
        assert_eq!(by_client.server_id, "server-dupe-1");

        let batch = repo
            .get_by_client_msg_ids(&["client-dupe-1".to_string()])
            .await
            .unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].server_id, "server-dupe-1");

        let timeline = repo.get_by_conversation("conv-dupe", 0, 10).await.unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].server_id, "server-dupe-1");
    }

    #[tokio::test]
    async fn update_after_ack_collapses_noncanonical_same_client_msg_id_row() {
        let repo = make_repo().await;
        let mut stale = IMMessage::new(flare_proto::common::Message::default());
        stale.server_id = "stale-local-ack-1".to_string();
        stale.client_msg_id = "client-ack-dupe-1".to_string();
        stale.conversation_id = "conv-ack-dupe".to_string();
        stale.sender_id = "u1".to_string();
        stale.local_state.sending = true;
        stale.local_state.is_local = true;
        repo.save_batch(&[stale.clone()]).await.unwrap();

        let mut acked = stale;
        acked.server_id = "server-ack-dupe-1".to_string();
        acked.conversation_seq = 21;
        acked.status = MessageStatus::Persisted as i32;

        repo.update_after_ack("client-ack-dupe-1", &acked)
            .await
            .unwrap();

        assert!(repo.get("stale-local-ack-1").await.unwrap().is_none());
        let stored = repo.get("server-ack-dupe-1").await.unwrap().unwrap();
        assert_eq!(stored.client_msg_id, "client-ack-dupe-1");
        assert_eq!(stored.status, MessageStatus::Sent as i32);

        let timeline = repo
            .get_by_conversation("conv-ack-dupe", 0, 10)
            .await
            .unwrap();
        assert_eq!(timeline.len(), 1);
        assert_eq!(timeline[0].server_id, "server-ack-dupe-1");
    }

    #[tokio::test]
    async fn latest_window_uses_seq_over_skewed_sort_ts_after_ack() {
        let repo = make_repo().await;
        let old = text_message("server-clock-old", "conv-clock", "u1", 10, 1_000, "old");
        let newest = text_message("server-clock-new", "conv-clock", "u2", 11, 2_000, "new");
        let mut pending = text_message("", "conv-clock", "u1", 0, 3_000, "pending");
        pending.client_msg_id = "client-clock-pending".to_string();
        pending.local_state.sending = true;
        pending.local_state.is_local = true;
        pending.local_state.sort_ts = 3_000;

        repo.save_batch(&[old, newest, pending]).await.unwrap();
        sqlx::query("UPDATE messages SET sort_ts = ? WHERE server_id = ?")
            .bind(99_999_i64)
            .bind("server-clock-old")
            .execute(&repo.pool)
            .await
            .unwrap();

        let latest = repo.get_by_conversation("conv-clock", 0, 3).await.unwrap();
        let ids = latest
            .iter()
            .map(|m| m.server_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["", "server-clock-new", "server-clock-old"]);
    }

    #[tokio::test]
    async fn reconcile_outgoing_read_by_peer_seq_downgrades_polluted_tail() {
        let repo = make_repo().await;
        let mut first = IMMessage::new(flare_proto::common::Message::default());
        first.server_id = "server-read-1".to_string();
        first.client_msg_id = "client-read-1".to_string();
        first.conversation_id = "conv-read".to_string();
        first.sender_id = "u1".to_string();
        first.conversation_seq = 1;
        first.status = MessageStatus::Sent as i32;
        first.is_read = true;

        let mut polluted_tail = first.clone();
        polluted_tail.server_id = "server-read-2".to_string();
        polluted_tail.client_msg_id = "client-read-2".to_string();
        polluted_tail.conversation_seq = 2;

        let mut other_sender = first.clone();
        other_sender.server_id = "server-read-other".to_string();
        other_sender.client_msg_id = "client-read-other".to_string();
        other_sender.sender_id = "u2".to_string();
        other_sender.conversation_seq = 3;

        repo.save_batch(&[first, polluted_tail, other_sender])
            .await
            .unwrap();

        repo.reconcile_outgoing_read_by_peer_seq("conv-read", "u1", 1)
            .await
            .unwrap();

        let first = repo.get("server-read-1").await.unwrap().unwrap();
        let tail = repo.get("server-read-2").await.unwrap().unwrap();
        let other = repo.get("server-read-other").await.unwrap().unwrap();
        assert_eq!(first.status, MessageStatus::Sent as i32);
        assert!(first.is_read);
        assert_eq!(tail.status, MessageStatus::Sent as i32);
        assert!(!tail.is_read);
        assert_eq!(other.status, MessageStatus::Sent as i32);
        assert!(other.is_read);
    }

    #[tokio::test]
    async fn apply_edit_event_ignores_stale_version_and_accepts_newer_version() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-1".to_string();
        message.client_msg_id = "client-1".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        message.encoded_content = b"old".to_vec();
        message
            .attributes
            .insert("currentEditVersion".to_string(), "2".to_string());
        repo.save_batch(&[message.clone()]).await.unwrap();

        let stale = repo
            .apply_edit_event("server-1", b"stale".to_vec(), 1)
            .await
            .unwrap();
        assert_eq!(stale, EditApplyResult::IgnoredStale);
        let after_stale = repo.get("server-1").await.unwrap().unwrap();
        assert_eq!(after_stale.encoded_content, b"old".to_vec());

        let newer = repo
            .apply_edit_event("server-1", b"new".to_vec(), 3)
            .await
            .unwrap();
        assert_eq!(newer, EditApplyResult::Applied);
        let after_new = repo.get("server-1").await.unwrap().unwrap();
        assert_eq!(after_new.encoded_content, b"new".to_vec());
        assert_eq!(
            after_new
                .attributes
                .get("currentEditVersion")
                .map(String::as_str),
            Some("3")
        );
    }

    #[tokio::test]
    async fn apply_pin_event_ignores_stale_seq_and_accepts_newer_seq() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-2".to_string();
        message.client_msg_id = "client-2".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_pin_event("server-2", true, Some(10))
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_pin_event("server-2", false, Some(9))
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let after_stale = repo.get("server-2").await.unwrap().unwrap();
        assert_eq!(
            after_stale.attributes.get("pinned").map(String::as_str),
            Some("true")
        );

        let newer = repo
            .apply_pin_event("server-2", false, Some(11))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);
        let after_new = repo.get("server-2").await.unwrap().unwrap();
        assert_eq!(
            after_new.attributes.get("pinned").map(String::as_str),
            Some("false")
        );
        assert_eq!(
            after_new
                .attributes
                .get("lastPinEventSeq")
                .map(String::as_str),
            Some("11")
        );
    }

    #[tokio::test]
    async fn save_batch_preserves_pin_event_attributes_when_snapshot_omits_them() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-2-persist".to_string();
        message.client_msg_id = "client-2-persist".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message.clone()]).await.unwrap();

        let applied = repo
            .apply_pin_event("server-2-persist", true, Some(10))
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let mut snapshot = message.clone();
        snapshot.attributes.clear();
        snapshot.updated_at = 20;
        repo.save_batch(&[snapshot]).await.unwrap();

        let after_snapshot = repo.get("server-2-persist").await.unwrap().unwrap();
        assert_eq!(
            after_snapshot.attributes.get("pinned").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            after_snapshot
                .attributes
                .get("lastPinEventSeq")
                .map(String::as_str),
            Some("10")
        );

        let unapplied = repo
            .apply_pin_event("server-2-persist", false, Some(11))
            .await
            .unwrap();
        assert_eq!(unapplied, OperationApplyResult::Applied);

        let mut second_snapshot = message;
        second_snapshot.attributes.clear();
        second_snapshot.updated_at = 30;
        repo.save_batch(&[second_snapshot]).await.unwrap();

        let after_unpin_snapshot = repo.get("server-2-persist").await.unwrap().unwrap();
        assert_eq!(
            after_unpin_snapshot
                .attributes
                .get("pinned")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(
            after_unpin_snapshot
                .attributes
                .get("lastPinEventSeq")
                .map(String::as_str),
            Some("11")
        );
    }

    #[tokio::test]
    async fn apply_mark_event_ignores_stale_seq_and_accepts_newer_seq() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-3".to_string();
        message.client_msg_id = "client-3".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_mark_event("server-3", 7, Some("#ff0000"), true, Some(20))
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_mark_event("server-3", 7, None, false, Some(19))
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let after_stale = repo.get("server-3").await.unwrap().unwrap();
        assert_eq!(
            after_stale.attributes.get("markType").map(String::as_str),
            Some("7")
        );
        assert_eq!(
            after_stale.attributes.get("markColor").map(String::as_str),
            Some("#ff0000")
        );

        let newer = repo
            .apply_mark_event("server-3", 7, None, false, Some(21))
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);

        let after_new = repo.get("server-3").await.unwrap().unwrap();
        assert!(!after_new.attributes.contains_key("markType"));
        assert!(!after_new.attributes.contains_key("markColor"));
        assert_eq!(
            after_new
                .attributes
                .get("lastMarkEventSeq:7")
                .map(String::as_str),
            Some("21")
        );
    }

    #[tokio::test]
    async fn apply_reaction_event_ignores_stale_seq_and_accepts_newer_seq() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-4".to_string();
        message.client_msg_id = "client-4".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        let applied = repo
            .apply_reaction_event(
                "conv-1",
                "server-4",
                "u2",
                "👍",
                ReactionAction::Add as i32,
                Some(30),
            )
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let stale = repo
            .apply_reaction_event(
                "conv-1",
                "server-4",
                "u2",
                "👍",
                ReactionAction::Remove as i32,
                Some(29),
            )
            .await
            .unwrap();
        assert_eq!(stale, OperationApplyResult::IgnoredStale);

        let reactions = repo
            .list_reactions(&["server-4".to_string()])
            .await
            .unwrap();
        assert_eq!(
            reactions
                .get("server-4")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );

        let newer = repo
            .apply_reaction_event(
                "conv-1",
                "server-4",
                "u2",
                "👍",
                ReactionAction::Remove as i32,
                Some(31),
            )
            .await
            .unwrap();
        assert_eq!(newer, OperationApplyResult::Applied);

        let reactions_after_remove = repo
            .list_reactions(&["server-4".to_string()])
            .await
            .unwrap();
        assert!(!reactions_after_remove.contains_key("server-4"));
    }

    #[tokio::test]
    async fn save_batch_without_reaction_snapshot_keeps_existing_reactions() {
        let repo = make_repo().await;
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-5".to_string();
        message.client_msg_id = "client-5".to_string();
        message.conversation_id = "conv-1".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        repo.apply_reaction_event(
            "conv-1",
            "server-5",
            "u2",
            "👍",
            ReactionAction::Add as i32,
            Some(1),
        )
        .await
        .unwrap();
        let before = repo
            .list_reactions(&["server-5".to_string()])
            .await
            .unwrap();
        assert_eq!(
            before
                .get("server-5")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );

        // 模拟同步下行：消息不带 reactions 快照（attributes 无 reactionsJson、reactions 为空）。
        let mut sync_message = IMMessage::new(flare_proto::common::Message::default());
        sync_message.server_id = "server-5".to_string();
        sync_message.client_msg_id = "client-5".to_string();
        sync_message.conversation_id = "conv-1".to_string();
        sync_message.sender_id = "u1".to_string();
        repo.save_batch(&[sync_message]).await.unwrap();

        let after = repo
            .list_reactions(&["server-5".to_string()])
            .await
            .unwrap();
        assert_eq!(
            after
                .get("server-5")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );
    }

    #[tokio::test]
    async fn apply_reaction_event_before_message_arrival_is_not_lost() {
        let repo = make_repo().await;

        // 先收到 reaction 事件（消息主体尚未落库）
        let applied = repo
            .apply_reaction_event(
                "conv-9",
                "server-9",
                "u9",
                "👍",
                ReactionAction::Add as i32,
                Some(9),
            )
            .await
            .unwrap();
        assert_eq!(applied, OperationApplyResult::Applied);

        let before = repo
            .list_reactions(&["server-9".to_string()])
            .await
            .unwrap();
        assert_eq!(
            before
                .get("server-9")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );

        // 后续消息同步到本地
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "server-9".to_string();
        message.client_msg_id = "client-9".to_string();
        message.conversation_id = "conv-9".to_string();
        message.sender_id = "u1".to_string();
        repo.save_batch(&[message]).await.unwrap();

        // 反应仍可通过消息 ID 聚合读取，确保 UI 可展示
        let after = repo
            .list_reactions(&["server-9".to_string()])
            .await
            .unwrap();
        assert_eq!(
            after
                .get("server-9")
                .and_then(|entries| entries.iter().find(|entry| entry.emoji == "👍"))
                .map(|entry| entry.count),
            Some(1)
        );
    }
}
