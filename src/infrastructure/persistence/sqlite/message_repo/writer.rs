use super::*;
use crate::shared::util::spawn_background;

const INLINE_FTS_BATCH_LIMIT: usize = 32;

#[derive(Clone)]
struct MessageFtsBackfillRow {
    server_id: String,
    conversation_id: String,
    search_text: Option<String>,
}

impl MessageFtsBackfillRow {
    fn from_persist_row(row: &MessagePersistRow<'_>) -> Self {
        Self {
            server_id: row.message.server_id.clone(),
            conversation_id: row.message.conversation_id.clone(),
            search_text: row.search_text.clone(),
        }
    }
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
            remove_conflicting_local_client_msg_rows_tx(&mut tx, &m.client_msg_id, &m.server_id)
                .await?;

            let server_id = m.server_id.trim();
            if !server_id.is_empty() {
                rows.retain(|row: &MessagePersistRow<'_>| {
                    let row_server_id = row.message.server_id.trim();
                    row_server_id != server_id
                });
            }
            rows.push(MessagePersistRow::from_message(m));
        }

        let mut latest_per_conversation: HashMap<&str, &IMMessage> = HashMap::new();
        let should_update_fts_inline = rows.len() <= INLINE_FTS_BATCH_LIMIT;
        for chunk in rows.chunks(MESSAGE_SAVE_BATCH_INSERT_CHUNK_SIZE) {
            insert_message_rows_tx(&mut tx, chunk).await?;
            if should_update_fts_inline {
                upsert_message_fts_rows_tx(&mut tx, chunk).await?;
            }
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
        let deferred_fts_rows = if should_update_fts_inline {
            Vec::new()
        } else {
            rows.iter()
                .map(MessageFtsBackfillRow::from_persist_row)
                .collect()
        };
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        if !deferred_fts_rows.is_empty() {
            schedule_message_fts_backfill(self.pool.clone(), deferred_fts_rows);
        }
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
               reply_to, quote_preview, thread_id, mention_users, mention_all, attributes, extensions, version, updated_at, text,
               sending, failed, is_local, sort_ts)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
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
        .bind(&message.thread_id)
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

fn schedule_message_fts_backfill(pool: SqlitePool, rows: Vec<MessageFtsBackfillRow>) {
    spawn_background(async move {
        let row_count = rows.len();
        if let Err(error) = backfill_message_fts_rows(pool, rows).await {
            tracing::warn!(
                %error,
                row_count,
                "message FTS backfill failed after large batch save"
            );
        }
    });
}

async fn backfill_message_fts_rows(
    pool: SqlitePool,
    rows: Vec<MessageFtsBackfillRow>,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut tx = pool.begin().await.map_err(sqlx_err)?;
    for chunk in rows.chunks(MESSAGE_SAVE_BATCH_INSERT_CHUNK_SIZE) {
        upsert_message_fts_backfill_rows_tx(&mut tx, chunk).await?;
        tokio::task::yield_now().await;
    }
    tx.commit().await.map_err(sqlx_err)?;
    Ok(())
}

async fn upsert_message_fts_backfill_rows_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[MessageFtsBackfillRow],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let mut delete_qb =
        QueryBuilder::<Sqlite>::new("DELETE FROM messages_fts WHERE server_id IN (");
    {
        let mut separated = delete_qb.separated(", ");
        for row in rows {
            separated.push_bind(&row.server_id);
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
        b.push_bind(&row.server_id)
            .push_bind(&row.conversation_id)
            .push_bind(text);
    });
    insert_qb
        .build()
        .execute(&mut **tx)
        .await
        .map_err(sqlx_err)?;
    Ok(())
}
