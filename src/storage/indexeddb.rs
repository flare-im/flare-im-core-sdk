//! IndexedDB 存储实现（Web）
//!
//! 基于 web-sys 和 wasm-bindgen 实现 IndexedDB 本地存储，支持消息、会话、同步游标和消息状态的持久化。

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use crate::model::{Message, SessionSummary, SyncCursor};
    use crate::storage::storage_trait::{
        LastMessageUpdate, MessageState, SessionFilter, SessionUpdate, StorageBackend,
    };
    use anyhow::{Context, Result};
    use async_trait::async_trait;
    use prost::Message as ProstMessage;
    use serde::{Deserialize, Serialize};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{IdbDatabase, IdbFactory, IdbIndex, IdbKeyRange, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransaction, IdbTransactionMode, IdbVersionChangeEvent};
    use tracing::{debug, error, info};

    /// IndexedDB 存储实现
    pub struct IndexedDBStorage {
        db: IdbDatabase,
        db_name: String,
    }

    impl crate::storage::storage_trait::StorageSyncBounds for IndexedDBStorage {}

    impl IndexedDBStorage {
        async fn await_open(request: IdbOpenDbRequest) -> Result<IdbDatabase> {
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                let resolve = resolve.clone();
                let reject = reject.clone();
                let success_cb = Closure::wrap(Box::new(move |event: web_sys::Event| {
                    if let Some(target) = event.target() {
                        if let Ok(req) = target.dyn_into::<IdbOpenDbRequest>() {
                            if let Ok(result) = req.result() {
                                let _ = resolve.call1(&JsValue::NULL, &result);
                                return;
                            }
                        }
                    }
                    let _ = resolve.call1(&JsValue::NULL, &JsValue::UNDEFINED);
                }) as Box<dyn FnMut(_)>);

                let error_cb = Closure::wrap(Box::new(move |event: web_sys::Event| {
                    if let Some(target) = event.target() {
                        if let Ok(req) = target.dyn_into::<IdbOpenDbRequest>() {
                            let err_js = req.error().map(|e| JsValue::from(e)).unwrap_or(JsValue::UNDEFINED);
                            let _ = reject.call1(&JsValue::NULL, &err_js);
                            return;
                        }
                    }
                    let _ = reject.call1(&JsValue::NULL, &JsValue::UNDEFINED);
                }) as Box<dyn FnMut(_)>);

                request.set_onsuccess(Some(success_cb.as_ref().unchecked_ref()));
                request.set_onerror(Some(error_cb.as_ref().unchecked_ref()));
                success_cb.forget();
                error_cb.forget();
            });

            let js = JsFuture::from(promise)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to await open: {:?}", e))?;

            js.dyn_into::<IdbDatabase>()
                .map_err(|e| anyhow::anyhow!("Failed to cast to IdbDatabase: {:?}", e))
        }
        async fn await_request(request: IdbRequest) -> Result<JsValue> {
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                let resolve = resolve.clone();
                let reject = reject.clone();
                let success_cb = Closure::wrap(Box::new(move |event: web_sys::Event| {
                    if let Some(target) = event.target() {
                        if let Ok(req) = target.dyn_into::<IdbRequest>() {
                            if let Ok(result) = req.result() {
                                let _ = resolve.call1(&JsValue::NULL, &result);
                                return;
                            }
                        }
                    }
                    let _ = resolve.call1(&JsValue::NULL, &JsValue::UNDEFINED);
                }) as Box<dyn FnMut(_)>);

                let error_cb = Closure::wrap(Box::new(move |event: web_sys::Event| {
                    if let Some(target) = event.target() {
                        if let Ok(req) = target.dyn_into::<IdbRequest>() {
                            let err_js = req.error().map(|e| JsValue::from(e)).unwrap_or(JsValue::UNDEFINED);
                            let _ = reject.call1(&JsValue::NULL, &err_js);
                            return;
                        }
                    }
                    let _ = reject.call1(&JsValue::NULL, &JsValue::UNDEFINED);
                }) as Box<dyn FnMut(_)>);

                request.set_onsuccess(Some(success_cb.as_ref().unchecked_ref()));
                request.set_onerror(Some(error_cb.as_ref().unchecked_ref()));
                success_cb.forget();
                error_cb.forget();
            });

            JsFuture::from(promise)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to await request: {:?}", e))
        }
        /// 创建新的 IndexedDB 存储实例
        ///
        /// # 参数
        /// - `db_name`: 数据库名称
        ///
        /// # 返回
        /// - `Result<Self>`: 存储实例或错误
        pub async fn new(db_name: &str) -> Result<Self> {
            let window = web_sys::window()
                .context("Failed to get window object")?;
            
            let idb_factory_opt = window
                .indexed_db()
                .map_err(|e| anyhow::anyhow!("Failed to get IndexedDB factory: {:?}", e))?;
            let idb_factory: IdbFactory = idb_factory_opt
                .ok_or_else(|| anyhow::anyhow!("IndexedDB not supported"))?;

            // 打开数据库（版本 2，支持索引优化）
            // 版本 1: 基础对象存储
            // 版本 2: 添加 session_id 索引，优化查询性能
            let open_request: IdbOpenDbRequest = idb_factory
                .open_with_u32(db_name, 2)
                .map_err(|e| anyhow::anyhow!("Failed to open database: {:?}", e))?;

            // 设置 onupgradeneeded 回调来创建对象存储和索引
            {
                let closure = Closure::wrap(Box::new(move |event: web_sys::IdbVersionChangeEvent| {
                    if let Some(target) = event.target().and_then(|t| t.dyn_into::<IdbOpenDbRequest>().ok()) {
                        if let Ok(result) = target.result() {
                            if let Ok(db) = result.dyn_into::<IdbDatabase>() {
                                let old_version = event.old_version();
                                let new_version = event.new_version();
                                
                                // 创建或获取对象存储
                                let messages_store = if old_version < 1 {
                                    // 新数据库，创建对象存储
                                    db.create_object_store("messages")
                                        .map_err(|e| {
                                            tracing::error!(error = ?e, "Failed to create messages store");
                                            e
                                        })
                                } else {
                                    // 已存在，获取事务中的对象存储
                                    let tx = event.transaction().ok_or_else(|| {
                                        js_sys::Error::new("No transaction available")
                                    })?;
                                    tx.object_store("messages")
                                        .map_err(|e| {
                                            tracing::error!(error = ?e, "Failed to get messages store");
                                            e
                                        })
                                };
                                
                                // 创建索引（仅在版本升级时）
                                if let Ok(store) = messages_store {
                                    if old_version < 2 {
                                        // 版本 2: 添加 session_id 索引
                                        if let Err(e) = store.create_index("session_id", "session_id", None) {
                                            tracing::warn!(error = ?e, "Failed to create session_id index (may already exist)");
                                        } else {
                                            tracing::info!("Created session_id index for messages store");
                                        }
                                    }
                                }
                                
                                // 创建其他对象存储（如果不存在）
                                if old_version < 1 {
                                    let _ = db.create_object_store("sessions");
                                    let _ = db.create_object_store("sync_cursors");
                                    let _ = db.create_object_store("message_states");
                                }
                            }
                        }
                    }
                }) as Box<dyn FnMut(web_sys::IdbVersionChangeEvent)>);
                
                open_request.set_onupgradeneeded(Some(closure.as_ref().unchecked_ref()));
                closure.forget(); // 注意：实际应该管理生命周期
            }

            // 等待数据库打开
            let db: IdbDatabase = Self::await_open(open_request)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to open database: {:?}", e))?;

            let storage = Self {
                db: db.clone(),
                db_name: db_name.to_string(),
            };

            info!(db_name = %db_name, "IndexedDB storage initialized");
            Ok(storage)
        }

        /// 获取对象存储
        fn get_object_store(
            &self,
            store_name: &str,
            mode: IdbTransactionMode,
        ) -> Result<IdbObjectStore> {
            let tx = self.db
                .transaction_with_str_and_mode(store_name, mode)
                .map_err(|e| anyhow::anyhow!("Failed to create transaction: {:?}", e))?;
            
            tx.object_store(store_name)
                .map_err(|e| anyhow::anyhow!("Failed to get object store: {:?}", e))
        }

        /// 序列化消息为 JSON
        fn serialize_message(message: &Message) -> Result<JsValue> {
            // 将 Message 序列化为 Protobuf 字节，然后转换为 base64 字符串存储
            let bytes = ProstMessage::encode_to_vec(message);
            use base64::Engine;
            let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"id".into(), &message.id.clone().into())
                .map_err(|e| anyhow::anyhow!("Failed to set id: {:?}", e))?;
            js_sys::Reflect::set(&obj, &"data".into(), &base64.into())
                .map_err(|e| anyhow::anyhow!("Failed to set data: {:?}", e))?;
            
            Ok(obj.into())
        }

        /// 反序列化消息
        fn deserialize_message(value: &JsValue) -> Result<Message> {
            let obj = value.dyn_ref::<js_sys::Object>()
                .context("Failed to cast to Object")?;
            
            let base64: String = js_sys::Reflect::get(obj, &"data".into())
                .map_err(|e| anyhow::anyhow!("Failed to get data field: {:?}", e))?
                .as_string()
                .context("Failed to get data field")?;
            
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(&base64)
                .context("Failed to decode base64")?;
            
            Message::decode(&bytes[..])
                .context("Failed to decode message")
        }
    }

    #[async_trait(?Send)]
    impl StorageBackend for IndexedDBStorage {
        // ========== 消息操作 ==========

        async fn save_message(&self, message: &Message) -> Result<()> {
            let store = self.get_object_store("messages", IdbTransactionMode::Readwrite)?;
            let value = Self::serialize_message(message)?;
            
            store.put_with_key(&JsValue::from_str(&message.id), &value)
                .map_err(|e| anyhow::anyhow!("Failed to put message: {:?}", e))?;
            
            Ok(())
        }

        async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
            let store = self.get_object_store("messages", IdbTransactionMode::Readonly)?;
            let request = store.get(&JsValue::from_str(message_id))
                .map_err(|e| anyhow::anyhow!("Failed to get message: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get message result: {:?}", e))?;
            
            if result.is_null() || result.is_undefined() {
                return Ok(None);
            }
            
            let message = Self::deserialize_message(&result)?;
            Ok(Some(message))
        }

        async fn get_messages(
            &self,
            session_id: &str,
            limit: usize,
            cursor: Option<String>,
        ) -> Result<Vec<Message>> {
            // 优化：使用 session_id 索引查询，大幅提升性能
            let store = self.get_object_store("messages", IdbTransactionMode::Readonly)?;
            
            // 尝试使用索引查询（如果索引存在）
            if let Ok(index) = store.index("session_id") {
                // 使用索引查询（优化路径）
                let key_range = IdbKeyRange::only(&JsValue::from_str(session_id))
                    .map_err(|e| anyhow::anyhow!("Failed to create key range: {:?}", e))?;
                
                let request = index.get_all(Some(&key_range))
                    .map_err(|e| anyhow::anyhow!("Failed to query index: {:?}", e))?;
                
                let result = Self::await_request(request).await
                    .map_err(|e| anyhow::anyhow!("Failed to get messages from index: {:?}", e))?;
                
                let array = js_sys::Array::from(&result);
                // 优化：预分配容量
                let mut messages = Vec::with_capacity(limit.min(array.length() as usize));
                
                for i in 0..array.length() {
                    let value = array.get(i);
                    match Self::deserialize_message(&value) {
                        Ok(msg) => {
                            messages.push(msg);
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to deserialize message");
                        }
                    }
                }
                
                // 按时间戳排序（最新的在前）
                messages.sort_by(|a, b| {
                    let ts_a = a.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0);
                    let ts_b = b.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0);
                    ts_b.cmp(&ts_a)
                });
                
                // 应用 limit
                messages.truncate(limit);
                
                return Ok(messages);
            }
            
            // 回退方案：全表扫描（向后兼容，或索引不存在时）
            let request = store.get_all()
                .map_err(|e| anyhow::anyhow!("Failed to get all messages: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get messages result: {:?}", e))?;
            
            let array = js_sys::Array::from(&result);
            // 优化：预分配容量
            let mut messages = Vec::with_capacity(limit.min(array.length() as usize));
            
            for i in 0..array.length() {
                let value = array.get(i);
                match Self::deserialize_message(&value) {
                    Ok(msg) if msg.session_id == session_id => {
                        messages.push(msg);
                    }
                    Ok(_) => {} // 不同会话的消息，跳过
                    Err(e) => {
                        error!(error = %e, "Failed to deserialize message");
                    }
                }
            }
            
            // 按时间戳排序（最新的在前）
            messages.sort_by(|a, b| {
                let ts_a = a.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0);
                let ts_b = b.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0);
                ts_b.cmp(&ts_a)
            });
            
            // 应用 limit
            messages.truncate(limit);
            
            Ok(messages)
        }

        async fn get_messages_by_seq(
            &self,
            session_id: &str,
            after_seq: i64,
            limit: usize,
        ) -> Result<Vec<Message>> {
            // 简化实现：获取所有消息，然后过滤
            let all_messages = self.get_messages(session_id, 10000, None).await?;
            
            let mut filtered: Vec<Message> = all_messages
                .into_iter()
                .filter(|msg| {
                    // 从 extra 中提取 seq
                    msg.extra
                        .get("seq")
                        .and_then(|v| v.parse::<i64>().ok())
                        .map(|seq| seq > after_seq)
                        .unwrap_or(false)
                })
                .collect();
            
            // 按 seq 排序
            filtered.sort_by(|a, b| {
                let seq_a = a.extra.get("seq").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                let seq_b = b.extra.get("seq").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                seq_a.cmp(&seq_b)
            });
            
            filtered.truncate(limit);
            Ok(filtered)
        }

        async fn get_max_seq(&self, session_id: &str) -> Result<Option<i64>> {
            let messages = self.get_messages(session_id, 10000, None).await?;
            
            let max_seq = messages
                .into_iter()
                .filter_map(|msg| {
                    msg.extra
                        .get("seq")
                        .and_then(|v| v.parse::<i64>().ok())
                })
                .max();
            
            Ok(max_seq)
        }

        async fn delete_message(&self, message_id: &str) -> Result<()> {
            let store = self.get_object_store("messages", IdbTransactionMode::Readwrite)?;
            store.delete(&JsValue::from_str(message_id))
                .map_err(|e| anyhow::anyhow!("Failed to delete message: {:?}", e))?;
            
            Ok(())
        }

        // ========== 会话操作 ==========

        async fn save_session(&self, session: &SessionSummary) -> Result<()> {
            let store = self.get_object_store("sessions", IdbTransactionMode::Readwrite)?;
            let value = serde_wasm_bindgen::to_value(session)
                .map_err(|e| anyhow::anyhow!("Failed to serialize session: {:?}", e))?;
            
            store.put_with_key(&JsValue::from_str(&session.session_id), &value)
                .map_err(|e| anyhow::anyhow!("Failed to put session: {:?}", e))?;
            
            Ok(())
        }

        async fn get_session(&self, session_id: &str) -> Result<Option<SessionSummary>> {
            let store = self.get_object_store("sessions", IdbTransactionMode::Readonly)?;
            let request = store.get(&JsValue::from_str(session_id))
                .map_err(|e| anyhow::anyhow!("Failed to get session: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get session result: {:?}", e))?;
            
            if result.is_null() || result.is_undefined() {
                return Ok(None);
            }
            
            let session: SessionSummary = serde_wasm_bindgen::from_value(result)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize session: {:?}", e))?;
            
            Ok(Some(session))
        }

        async fn get_sessions(&self, filter: SessionFilter) -> Result<Vec<SessionSummary>> {
            let store = self.get_object_store("sessions", IdbTransactionMode::Readonly)?;
            let request = store.get_all()
                .map_err(|e| anyhow::anyhow!("Failed to get all sessions: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get sessions result: {:?}", e))?;
            
            let array = js_sys::Array::from(&result);
            let mut sessions = Vec::new();
            
            for i in 0..array.length() {
                let value = array.get(i);
                    match serde_wasm_bindgen::from_value::<SessionSummary>(value.clone()) {
                        Ok(session) => {
                            // 应用过滤条件
                            let matches = (filter.session_type.is_none() || filter.session_type.as_ref() == Some(&session.session_type))
                                && (filter.business_type.is_none() || filter.business_type.as_ref() == Some(&session.business_type))
                                && (!filter.unread_only || session.unread_count > 0);
                            
                            if matches {
                                sessions.push(session);
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to deserialize session");
                        }
                    }
            }
            
            // 按最后消息时间排序（最新的在前）
            sessions.sort_by(|a, b| {
                let ts_a = a.last_message_time.unwrap_or(0);
                let ts_b = b.last_message_time.unwrap_or(0);
                ts_b.cmp(&ts_a)
            });
            
            // 应用 limit 和 offset
            if let Some(limit) = filter.limit {
                let start = filter.offset.unwrap_or(0);
                let end = start + limit;
                if end < sessions.len() {
                    sessions = sessions[start..end].to_vec();
                } else if start < sessions.len() {
                    sessions = sessions[start..].to_vec();
                } else {
                    sessions.clear();
                }
            }
            
            Ok(sessions)
        }

        async fn update_session(
            &self,
            session_id: &str,
            updates: SessionUpdate,
        ) -> Result<()> {
            // 获取现有会话
            let mut session = self.get_session(session_id).await?
                .context("Session not found")?;
            
            // 应用更新
            if let Some(ref last_msg) = updates.last_message {
                session.last_message_id = Some(last_msg.message_id.clone());
                session.last_message_time = Some(last_msg.message_time);
                session.last_sender_id = last_msg.sender_id.clone();
                session.last_message_type = last_msg.message_type;
                session.last_content_type = last_msg.content_type.clone();
            }
            
            if let Some(unread_count) = updates.unread_count {
                session.unread_count = unread_count;
            }
            
            if let Some(ref display_name) = updates.display_name {
                session.display_name = Some(display_name.clone());
            }
            
            if let Some(ref metadata) = updates.metadata {
                session.metadata = metadata.clone();
            }
            
            // 保存更新后的会话
            self.save_session(&session).await
        }

        async fn delete_session(&self, session_id: &str) -> Result<()> {
            let store = self.get_object_store("sessions", IdbTransactionMode::Readwrite)?;
            store.delete(&JsValue::from_str(session_id))
                .map_err(|e| anyhow::anyhow!("Failed to delete session: {:?}", e))?;
            
            Ok(())
        }

        // ========== 同步游标操作 ==========

        async fn save_sync_cursor(&self, session_id: &str, cursor: &SyncCursor) -> Result<()> {
            let store = self.get_object_store("sync_cursors", IdbTransactionMode::Readwrite)?;
            let value = serde_wasm_bindgen::to_value(cursor)
                .map_err(|e| anyhow::anyhow!("Failed to serialize sync cursor: {:?}", e))?;
            
            store.put_with_key(&JsValue::from_str(session_id), &value)
                .map_err(|e| anyhow::anyhow!("Failed to put sync cursor: {:?}", e))?;
            
            Ok(())
        }

        async fn get_sync_cursor(&self, session_id: &str) -> Result<Option<SyncCursor>> {
            let store = self.get_object_store("sync_cursors", IdbTransactionMode::Readonly)?;
            let request = store.get(&JsValue::from_str(session_id))
                .map_err(|e| anyhow::anyhow!("Failed to get sync cursor: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get sync cursor result: {:?}", e))?;
            
            if result.is_null() || result.is_undefined() {
                return Ok(None);
            }
            
            let cursor: SyncCursor = serde_wasm_bindgen::from_value(result)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize sync cursor: {:?}", e))?;
            
            Ok(Some(cursor))
        }

        async fn get_all_sync_cursors(&self) -> Result<Vec<SyncCursor>> {
            let store = self.get_object_store("sync_cursors", IdbTransactionMode::Readonly)?;
            let request = store.get_all()
                .map_err(|e| anyhow::anyhow!("Failed to get all sync cursors: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get sync cursors result: {:?}", e))?;
            
            let array = js_sys::Array::from(&result);
            let mut cursors = Vec::new();
            
            for i in 0..array.length() {
                let value = array.get(i);
                    match serde_wasm_bindgen::from_value::<SyncCursor>(value.clone()) {
                        Ok(cursor) => cursors.push(cursor),
                        Err(e) => {
                            error!(error = %e, "Failed to deserialize sync cursor");
                        }
                    }
            }
            
            Ok(cursors)
        }

        // ========== 消息状态操作 ==========

        async fn save_message_state(
            &self,
            user_id: &str,
            message_id: &str,
            state: MessageState,
        ) -> Result<()> {
            let store = self.get_object_store("message_states", IdbTransactionMode::Readwrite)?;
            
            // 使用复合键：user_id:message_id
            let key = format!("{}:{}", user_id, message_id);
            let value = serde_wasm_bindgen::to_value(&state)
                .map_err(|_e| anyhow::anyhow!("Failed to serialize message state"))?;
            
            store.put_with_key(&JsValue::from_str(&key), &value)
                .map_err(|e| anyhow::anyhow!("Failed to put message state: {:?}", e))?;
            
            Ok(())
        }

        async fn get_message_state(
            &self,
            user_id: &str,
            message_id: &str,
        ) -> Result<Option<MessageState>> {
            let store = self.get_object_store("message_states", IdbTransactionMode::Readonly)?;
            
            let key = format!("{}:{}", user_id, message_id);
            let request = store.get(&JsValue::from_str(&key))
                .map_err(|e| anyhow::anyhow!("Failed to get message state: {:?}", e))?;
            
            let result = Self::await_request(request).await
                .map_err(|e| anyhow::anyhow!("Failed to get message state result: {:?}", e))?;
            
            if result.is_null() || result.is_undefined() {
                return Ok(None);
            }
            
            let state: MessageState = serde_wasm_bindgen::from_value(result)
                .map_err(|_e| anyhow::anyhow!("Failed to deserialize message state"))?;
            
            Ok(Some(state))
        }

        async fn batch_check_deleted(
            &self,
            user_id: &str,
            message_ids: &[String],
        ) -> Result<Vec<String>> {
            let mut deleted_ids = Vec::new();
            
            for message_id in message_ids {
                if let Some(state) = self.get_message_state(user_id, message_id).await? {
                    if state.is_deleted {
                        deleted_ids.push(message_id.clone());
                    }
                }
            }
            
            Ok(deleted_ids)
        }
    }
}

// 只在 wasm32 目标下编译
#[cfg(target_arch = "wasm32")]
pub use wasm_impl::IndexedDBStorage;
