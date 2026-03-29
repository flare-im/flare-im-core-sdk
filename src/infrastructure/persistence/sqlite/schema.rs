//! SQLite 表结构初始化，供 `create_pool` 后调用。

use crate::error::{ErrorCode, FlareError, Result};
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

    Ok(())
}
