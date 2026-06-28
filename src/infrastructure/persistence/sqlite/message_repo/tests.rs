use super::SqliteMessageRepo;
use crate::domain::{
    ConversationReader, ConversationWriter, EditApplyResult, MessageReader, MessageStore,
    MessageWriter, OperationApplyResult,
};
use crate::infrastructure::persistence::sqlite::conversation_repo::SqliteConversationRepo;
use crate::infrastructure::persistence::sqlite_init_schema;
use crate::model::conversation::ConversationType;
use crate::model::message::{MessageStatus, ReactionAction};
use crate::model::{Conversation, IMMessage, MessageSearchKind, MessageSearchQuery, MessageType};
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
async fn apply_edit_event_ignores_stale_seq_and_accepts_newer_seq() {
    let repo = make_repo().await;
    let mut message = IMMessage::new(flare_proto::common::Message::default());
    message.server_id = "server-1".to_string();
    message.client_msg_id = "client-1".to_string();
    message.conversation_id = "conv-1".to_string();
    message.sender_id = "u1".to_string();
    message.encoded_content = b"old".to_vec();
    repo.save_batch(&[message.clone()]).await.unwrap();

    // 首次编辑（seq=100）应用。
    let first = repo
        .apply_edit_event("server-1", b"first".to_vec(), Some(100))
        .await
        .unwrap();
    assert_eq!(first, EditApplyResult::Applied);
    assert_eq!(
        repo.get("server-1").await.unwrap().unwrap().encoded_content,
        b"first".to_vec()
    );

    // 陈旧/重复（seq<=100）忽略，内容不变。
    let stale = repo
        .apply_edit_event("server-1", b"stale".to_vec(), Some(100))
        .await
        .unwrap();
    assert_eq!(stale, EditApplyResult::IgnoredStale);
    assert_eq!(
        repo.get("server-1").await.unwrap().unwrap().encoded_content,
        b"first".to_vec()
    );

    // 关键回归：更高 seq 的第二次编辑必须应用（修复前因 edit_version 恒为 1 被误判陈旧丢弃）。
    let newer = repo
        .apply_edit_event("server-1", b"second".to_vec(), Some(150))
        .await
        .unwrap();
    assert_eq!(newer, EditApplyResult::Applied);
    let after_new = repo.get("server-1").await.unwrap().unwrap();
    assert_eq!(after_new.encoded_content, b"second".to_vec());
    assert_eq!(
        after_new
            .attributes
            .get("messageFsmState")
            .map(String::as_str),
        Some("EDITED")
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
