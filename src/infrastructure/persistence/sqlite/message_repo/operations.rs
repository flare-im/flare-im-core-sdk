use super::*;

#[async_trait]
impl MessageStore for SqliteMessageRepo {
    async fn apply_edit_event(
        &self,
        message_id: &str,
        new_content: Vec<u8>,
        event_seq: Option<u64>,
    ) -> Result<EditApplyResult> {
        // 读改写必须同事务：SELECT（lastEditEventSeq 判定）与 UPDATE 之间若允许并发
        // 编辑事件交错，旧事件可能覆盖新事件。WAL 下并发写会以 BUSY 失败并走事件重放。
        let mut tx = self.pool.begin().await.map_err(sqlx_err)?;
        let row = sqlx::query(
            r#"SELECT attributes
               FROM messages
               WHERE server_id = ? OR client_msg_id = ?
               LIMIT 1"#,
        )
        .bind(message_id)
        .bind(message_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_err)?;

        let Some(row) = row else {
            return Ok(EditApplyResult::NotFound);
        };

        let extra_raw: Option<String> = row.try_get("attributes").map_err(sqlx_err)?;
        let mut attributes = parse_extra(extra_raw.as_deref());

        // 陈旧/乱序防护：用权威 event_seq（conversation_seq）判定，与反应路径一致。
        // 不再以 edit_version 排序——服务端无法在发布前同步分配版本，曾恒为 1，
        // 导致同一消息的第二次及以后编辑被误判陈旧、在客户端静默丢弃。
        if let Some(seq) = event_seq {
            let last_seq = attributes
                .get("lastEditEventSeq")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if seq <= last_seq {
                return Ok(EditApplyResult::IgnoredStale);
            }
            attributes.insert("lastEditEventSeq".to_string(), seq.to_string());
        }

        let now_ms = now_ms_i64();
        let text = text_for_sqlite_from_content_bytes(&new_content);
        let search_text = search_text_for_content_bytes(&new_content).or_else(|| text.clone());
        // currentEditVersion 仅作「已编辑」布尔信号（见 model::message::is_edited_from_attributes），
        // 精确计数对客户端无意义；本地单调自增即可，顺序已由 event_seq 保证。
        let current_edit_version = attributes
            .get("currentEditVersion")
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(0);
        attributes.insert(
            "currentEditVersion".to_string(),
            (current_edit_version + 1).to_string(),
        );
        attributes.insert("messageFsmState".to_string(), "EDITED".to_string());
        attributes.insert("lastEditedAt".to_string(), now_ms.to_string());

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
        // 读态单调:对端已读回执只前进（peer_read_seq 单调增），一旦某条被对方读过就恒为已读。
        // 早先这里会对 seq>peer_read_seq 的消息 is_read=0 回退——冷启/陈旧同步下发的偏低
        // （甚至 0，此时上面 mark 被跳过而 un-read 仍会抹掉全部已读）peer_read_seq 会把已读消息
        // 退回未读，表现为「双勾过一下变单勾」。只前进不回退，不再 un-read。
        if peer_read_seq > 0 {
            self.mark_outgoing_read_upto_seq(conversation_id, sender_user_id, peer_read_seq)
                .await?;
        }
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
