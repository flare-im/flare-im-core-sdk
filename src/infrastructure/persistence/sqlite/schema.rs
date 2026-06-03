//! SQLite 表结构初始化，供 `create_pool` 后调用。

use crate::shared::error::{ErrorCode, FlareError, Result};
use sqlx::SqlitePool;

/// 创建所有仓储所需表（messages / conversations / pending_sends / user_profiles / sync_cursors / sync_conversation_cursors）
pub async fn init_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS messages (
            server_id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            client_msg_id TEXT NOT NULL DEFAULT '',
            sender_id TEXT NOT NULL DEFAULT '',
            source INTEGER NOT NULL DEFAULT 0,
            seq INTEGER NOT NULL DEFAULT 0,
            timestamp INTEGER NOT NULL DEFAULT 0,
            client_timestamp INTEGER NOT NULL DEFAULT 0,
            conversation_type INTEGER NOT NULL DEFAULT 0,
            message_type INTEGER NOT NULL DEFAULT 0,
            channel_id TEXT NOT NULL DEFAULT '',
            sender_name TEXT NOT NULL DEFAULT '',
            sender_avatar TEXT NOT NULL DEFAULT '',
            sender_display_name TEXT NOT NULL DEFAULT '',
            content BLOB NOT NULL,
            status INTEGER NOT NULL DEFAULT 0,
            burn_enabled INTEGER NOT NULL DEFAULT 0,
            burn_after_read_seconds INTEGER,
            burn_status INTEGER NOT NULL DEFAULT 0,
            first_read_at INTEGER,
            burn_at INTEGER,
            burned_at INTEGER,
            is_read INTEGER NOT NULL DEFAULT 0,
            is_recalled INTEGER NOT NULL DEFAULT 0,
            is_edited INTEGER NOT NULL DEFAULT 0,
            reply_to TEXT,
            quote_preview TEXT,
            mention_users TEXT,
            mention_all INTEGER NOT NULL DEFAULT 0,
            extra TEXT,
            extensions TEXT,
            version INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            text TEXT,
            sending INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0,
            is_local INTEGER NOT NULL DEFAULT 0,
            sort_ts INTEGER NOT NULL DEFAULT 0
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_messages_conv_seq
           ON messages(conversation_id, seq DESC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_messages_client_msg_id
           ON messages(client_msg_id)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS message_reactions (
            message_server_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL DEFAULT '',
            emoji TEXT NOT NULL,
            user_id TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (message_server_id, emoji, user_id)
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_message_reactions_message
           ON message_reactions(message_server_id, updated_at DESC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_message_reactions_conversation
           ON message_reactions(conversation_id, updated_at DESC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS conversations (
            conversation_id TEXT PRIMARY KEY,
            conversation_type INTEGER NOT NULL,
            business_type TEXT NOT NULL,
            channel_id TEXT NOT NULL DEFAULT '',
            members_count INTEGER NOT NULL DEFAULT 0,
            display_name TEXT NOT NULL,
            avatar_url TEXT NOT NULL,
            remark TEXT,
            description TEXT,
            last_message_id TEXT,
            last_sender_id TEXT,
            last_message_at INTEGER,
            last_message_preview TEXT,
            last_sender_nickname TEXT NOT NULL DEFAULT '',
            last_sender_avatar_url TEXT NOT NULL DEFAULT '',
            unread_count INTEGER NOT NULL DEFAULT 0,
            last_read_seq INTEGER NOT NULL DEFAULT 0,
            max_seq INTEGER NOT NULL DEFAULT 0,
            visible_after_seq INTEGER NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            is_muted INTEGER NOT NULL DEFAULT 0,
            is_archived INTEGER NOT NULL DEFAULT 0,
            version INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at_ts INTEGER,
            ext TEXT,
            draft TEXT,
            mention_count INTEGER NOT NULL DEFAULT 0,
            mention_me INTEGER NOT NULL DEFAULT 0,
            badge TEXT,
            role TEXT
        );"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_conversations_sort
                ON conversations (
                    is_archived,
                    is_pinned DESC,
                    last_message_at DESC
                );"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_conversations_channel
           ON conversations (channel_id)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_conversations_last_message_id
           ON conversations (last_message_id)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS conversation_participants (
            conversation_id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            roles TEXT NOT NULL DEFAULT '[]',
            muted INTEGER NOT NULL DEFAULT 0,
            pinned INTEGER NOT NULL DEFAULT 0,
            attributes TEXT NOT NULL DEFAULT '{}',
            joined_at INTEGER NOT NULL DEFAULT 0,
            nickname TEXT NOT NULL DEFAULT '',
            participant_version INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (conversation_id, user_id)
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_conversation_participants_list
           ON conversation_participants (conversation_id, joined_at ASC, user_id ASC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS pending_sends (
            client_msg_id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            enqueued_at_ms INTEGER NOT NULL,
            data BLOB NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_pending_sends_enqueued_at_ms
           ON pending_sends (enqueued_at_ms ASC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    // 覆盖 (enqueued_at_ms, client_msg_id)，便于「队首」子查询尽量走索引、再按主键取 BLOB。
    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_pending_sends_queue_head
           ON pending_sends (enqueued_at_ms ASC, client_msg_id ASC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query("ANALYZE pending_sends")
        .execute(pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS user_profiles (
            user_id TEXT PRIMARY KEY,
            nickname TEXT NOT NULL DEFAULT '',
            avatar_url TEXT NOT NULL DEFAULT ''
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS sync_cursors (
            key TEXT PRIMARY KEY,
            cursor TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS sync_conversation_cursors (
            user_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL,
            last_seq INTEGER NOT NULL DEFAULT 0,
            synced_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (user_id, conversation_id)
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS media_upload_manifest (
            local_upload_id TEXT PRIMARY KEY,
            remote_upload_id TEXT,
            file_id TEXT,
            storage_upload_id TEXT,
            tenant_id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            source_locator TEXT NOT NULL,
            file_name TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            part_size INTEGER NOT NULL,
            total_parts INTEGER NOT NULL,
            transport_kind TEXT,
            bucket TEXT,
            object_key TEXT,
            upload_url TEXT,
            file_fingerprint TEXT NOT NULL,
            head_tail_sha256 TEXT NOT NULL,
            full_sha256 TEXT,
            upload_state TEXT NOT NULL,
            last_error_code TEXT,
            last_error_message TEXT,
            expires_at_ms INTEGER,
            created_at_ms INTEGER NOT NULL,
            updated_at_ms INTEGER NOT NULL
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_media_upload_manifest_active
           ON media_upload_manifest (source_locator, file_fingerprint, upload_state, updated_at_ms DESC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS media_local_cache (
            file_id TEXT PRIMARY KEY,
            local_path TEXT NOT NULL,
            mime_type TEXT NOT NULL DEFAULT '',
            size_bytes INTEGER NOT NULL DEFAULT 0,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_media_local_cache_updated
           ON media_local_cache (updated_at_ms DESC)"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS media_cache_settings (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            max_bytes INTEGER NOT NULL DEFAULT 0,
            cache_root TEXT NOT NULL DEFAULT ''
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"INSERT OR IGNORE INTO media_cache_settings (singleton, max_bytes, cache_root) VALUES (1, 0, '')"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS media_upload_part (
            local_upload_id TEXT NOT NULL,
            part_number INTEGER NOT NULL,
            offset_bytes INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            sha256 TEXT NOT NULL,
            etag TEXT,
            uploaded INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (local_upload_id, part_number)
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS user_file_download (
            download_key TEXT PRIMARY KEY,
            local_path TEXT NOT NULL,
            display_name TEXT NOT NULL DEFAULT '',
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS file_download_settings (
            singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
            download_subfolder TEXT NOT NULL DEFAULT 'flare'
        )"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    sqlx::query(
        r#"INSERT OR IGNORE INTO file_download_settings (singleton, download_subfolder) VALUES (1, 'flare')"#,
    )
    .execute(pool)
    .await
    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

    Ok(())
}
