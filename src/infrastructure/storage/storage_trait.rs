//! 存储抽象层
//!
//! 定义统一的存储接口，支持不同平台的存储实现（SQLite、IndexedDB）

#[cfg(feature = "extensions")]
use crate::domain::extension::{MessageExtension, SessionExtension};
use crate::domain::{Message, SessionSummary, SyncCursor};
use anyhow::Result;
use async_trait::async_trait;

/// 存储后端 trait
///
/// 为不同平台的存储实现提供统一接口
/// - 桌面端/移动端：SQLite
/// - Web 端：IndexedDB
#[cfg(not(target_arch = "wasm32"))]
pub trait StorageSyncBounds: Send + Sync {}

#[cfg(target_arch = "wasm32")]
pub trait StorageSyncBounds {}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[async_trait]
pub trait StorageBackend: StorageSyncBounds {
    // ========== 消息操作 ==========

    /// 保存消息
    async fn save_message(&self, message: &Message) -> Result<()>;

    /// 批量保存消息（事务优化，性能提升 10-50 倍）
    ///
    /// # 参数
    /// - `messages`: 要保存的消息列表
    ///
    /// # 返回
    /// - `Result<()>`: 保存结果
    ///
    /// # 性能优化
    /// - 使用事务批量插入，减少数据库往返
    /// - 对于大量消息，建议使用此方法而非多次调用 `save_message`
    ///
    /// # 默认实现
    /// 默认逐个保存（子类应重写以优化性能）
    async fn batch_save_messages(&self, messages: &[Message]) -> Result<()> {
        // 默认实现：逐个保存（子类可以优化为事务批量保存）
        for message in messages {
            self.save_message(message).await?;
        }
        Ok(())
    }

    /// 根据消息 ID 获取消息
    async fn get_message(&self, message_id: &str) -> Result<Option<Message>>;

    /// 批量获取消息（优化：减少数据库往返）
    ///
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    ///
    /// # 返回
    /// - 消息列表（按 message_ids 顺序，不存在的消息会被跳过）
    async fn batch_get_messages(&self, message_ids: &[String]) -> Result<Vec<Message>> {
        // 默认实现：逐个查询（子类可以优化）
        let mut messages = Vec::new();
        for message_id in message_ids {
            if let Ok(Some(message)) = self.get_message(message_id).await {
                messages.push(message);
            }
        }
        Ok(messages)
    }

    /// 获取会话消息列表（基于游标分页）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 返回的最大消息数量
    /// - `cursor`: 可选游标，用于分页（格式：`seq:<seq>:<message_id>`）
    ///
    /// # 返回
    /// - 消息列表（按时间倒序，最新的在前）
    async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<Message>>;

    /// 根据 seq 范围获取消息（用于增量同步）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `after_seq`: 起始 seq（不包含）
    /// - `limit`: 返回的最大消息数量
    ///
    /// # 返回
    /// - 消息列表（按 seq 升序）
    async fn get_messages_by_seq(
        &self,
        session_id: &str,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<Message>>;

    /// 获取会话的最大 seq
    async fn get_max_seq(&self, session_id: &str) -> Result<Option<i64>>;

    /// 删除消息（软删除，标记为已删除）
    async fn delete_message(&self, message_id: &str) -> Result<()>;

    /// 删除会话中的所有消息（仅本地，软删除）
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - 删除的消息数量
    async fn delete_all_messages(&self, session_id: &str) -> Result<usize> {
        // 默认实现：获取所有消息并逐个删除
        let messages = self.get_messages(session_id, usize::MAX, None).await?;
        let count = messages.len();
        for message in messages {
            self.delete_message(&message.id).await?;
        }
        Ok(count)
    }

    /// 搜索消息（本地搜索）
    ///
    /// # 参数
    /// - `query`: 搜索关键词（搜索文本内容）
    /// - `session_id`: 会话 ID（可选，如果提供则只搜索该会话）
    /// - `limit`: 最大返回数量
    ///
    /// # 返回
    /// - 匹配的消息列表
    async fn search_messages(
        &self,
        query: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        // 默认实现：获取所有消息并过滤（子类可以优化为数据库全文搜索）
        let mut results = Vec::new();

        if let Some(sid) = session_id {
            // 只搜索指定会话
            let messages = self.get_messages(sid, usize::MAX, None).await?;
            for msg in messages {
                // 使用默认实现检查消息是否匹配
                let matches = if let Some(content) = &msg.content {
                    if let Some(proto_content) = &content.content {
                        match proto_content {
                            flare_proto::flare::common::v1::message_content::Content::Text(
                                text_content,
                            ) => text_content
                                .text
                                .to_lowercase()
                                .contains(&query.to_lowercase()),
                            _ => false,
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if matches {
                    results.push(msg);
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        } else {
            // 搜索所有会话（需要遍历所有会话）
            // 注意：这是一个低效的默认实现，子类应该重写
            let sessions = self
                .get_sessions(crate::infrastructure::storage::SessionFilter::default())
                .await?;
            for session in sessions {
                let messages = self.get_messages(&session.session_id, 1000, None).await?;
                for msg in messages {
                    // 使用默认实现检查消息是否匹配
                    let matches = if let Some(content) = &msg.content {
                        if let Some(proto_content) = &content.content {
                            match proto_content {
                                flare_proto::flare::common::v1::message_content::Content::Text(
                                    text_content,
                                ) => text_content
                                    .text
                                    .to_lowercase()
                                    .contains(&query.to_lowercase()),
                                _ => false,
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if matches {
                        results.push(msg);
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
                if results.len() >= limit {
                    break;
                }
            }
        }

        Ok(results)
    }

    /// 根据条件查找消息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID（可选）
    /// - `sender_id`: 发送者 ID（可选）
    /// - `message_type`: 消息类型（可选）
    /// - `start_time`: 开始时间（可选，毫秒时间戳）
    /// - `end_time`: 结束时间（可选，毫秒时间戳）
    /// - `limit`: 最大返回数量
    ///
    /// # 返回
    /// - 匹配的消息列表
    async fn find_messages(
        &self,
        session_id: Option<&str>,
        sender_id: Option<&str>,
        message_type: Option<i32>,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        // 默认实现：获取消息并过滤（子类可以优化为数据库查询）
        let mut results = Vec::new();

        if let Some(sid) = session_id {
            let messages = self.get_messages(sid, usize::MAX, None).await?;
            for msg in messages {
                // 使用默认实现检查消息是否匹配过滤条件
                let mut matches = true;

                // 检查发送者
                if let Some(sid) = sender_id {
                    if msg.sender_id != sid {
                        matches = false;
                    }
                }

                // 检查消息类型
                if matches {
                    if let Some(mt) = message_type {
                        if msg.message_type != mt {
                            matches = false;
                        }
                    }
                }

                // 检查时间范围
                if matches {
                    let msg_time = msg
                        .timeline
                        .as_ref()
                        .and_then(|t| {
                            t.persisted_at
                                .as_ref()
                                .or(t.delivered_at.as_ref())
                                .or(t.created_at.as_ref())
                        })
                        .map(|ts| (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000)
                        .unwrap_or(0);

                    if let Some(start) = start_time {
                        if msg_time < start {
                            matches = false;
                        }
                    }

                    if matches {
                        if let Some(end) = end_time {
                            if msg_time > end {
                                matches = false;
                            }
                        }
                    }
                }

                if matches {
                    results.push(msg);
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        } else {
            // 搜索所有会话
            let sessions = self
                .get_sessions(crate::infrastructure::storage::SessionFilter::default())
                .await?;
            for session in sessions {
                let messages = self.get_messages(&session.session_id, 1000, None).await?;
                for msg in messages {
                    // 使用默认实现检查消息是否匹配过滤条件
                    let mut matches = true;

                    // 检查发送者
                    if let Some(sid) = sender_id {
                        if msg.sender_id != sid {
                            matches = false;
                        }
                    }

                    // 检查消息类型
                    if matches {
                        if let Some(mt) = message_type {
                            if msg.message_type != mt {
                                matches = false;
                            }
                        }
                    }

                    // 检查时间范围
                    if matches {
                        let msg_time = msg
                            .timeline
                            .as_ref()
                            .and_then(|t| t.persisted_at.as_ref().or(t.delivered_at.as_ref()))
                            .map(|ts| (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000)
                            .unwrap_or(0);

                        if let Some(start) = start_time {
                            if msg_time < start {
                                matches = false;
                            }
                        }

                        if matches {
                            if let Some(end) = end_time {
                                if msg_time > end {
                                    matches = false;
                                }
                            }
                        }
                    }

                    if matches {
                        results.push(msg);
                        if results.len() >= limit {
                            break;
                        }
                    }
                }
                if results.len() >= limit {
                    break;
                }
            }
        }

        Ok(results)
    }

    // ========== 辅助方法（用于默认实现） ==========

    /// 检查消息是否匹配搜索关键词（辅助方法）
    fn message_matches_query(&self, message: &Message, query: &str) -> bool
    where
        Self: Sized,
    {
        // 检查消息内容中的文本
        if let Some(content) = &message.content {
            if let Some(proto_content) = &content.content {
                match proto_content {
                    flare_proto::flare::common::v1::message_content::Content::Text(
                        text_content,
                    ) => {
                        if text_content
                            .text
                            .to_lowercase()
                            .contains(&query.to_lowercase())
                        {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }

    /// 检查消息是否匹配过滤条件（辅助方法）
    fn message_matches_filters(
        &self,
        message: &Message,
        sender_id: Option<&str>,
        message_type: Option<i32>,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> bool
    where
        Self: Sized,
    {
        // 检查发送者
        if let Some(sid) = sender_id {
            if message.sender_id != sid {
                return false;
            }
        }

        // 检查消息类型
        if let Some(mt) = message_type {
            if message.message_type != mt {
                return false;
            }
        }

        // 检查时间范围
        let msg_time = message
            .timeline
            .as_ref()
            .and_then(|t| t.persisted_at.as_ref().or(t.delivered_at.as_ref()))
            .map(|ts| (ts.seconds as i64) * 1000 + (ts.nanos as i64) / 1_000_000)
            .unwrap_or(0);

        if let Some(start) = start_time {
            if msg_time < start {
                return false;
            }
        }

        if let Some(end) = end_time {
            if msg_time > end {
                return false;
            }
        }

        true
    }

    // ========== 会话操作 ==========

    /// 保存会话
    async fn save_session(&self, session: &SessionSummary) -> Result<()>;

    /// 根据会话 ID 获取会话
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionSummary>>;

    /// 获取会话列表
    ///
    /// # 参数
    /// - `filter`: 过滤条件
    ///
    /// # 返回
    /// - 会话列表（按最后消息时间倒序）
    async fn get_sessions(&self, filter: SessionFilter) -> Result<Vec<SessionSummary>>;

    /// 批量获取会话（优化：减少数据库往返）
    ///
    /// # 参数
    /// - `session_ids`: 会话 ID 列表
    ///
    /// # 返回
    /// - 会话列表（按 session_ids 顺序，不存在的会话会被跳过）
    async fn batch_get_sessions(&self, session_ids: &[String]) -> Result<Vec<SessionSummary>> {
        // 默认实现：逐个查询（子类可以优化）
        let mut sessions = Vec::new();
        for session_id in session_ids {
            if let Ok(Some(session)) = self.get_session(session_id).await {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    /// 更新会话
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `updates`: 更新内容
    async fn update_session(&self, session_id: &str, updates: SessionUpdate) -> Result<()>;

    /// 删除会话
    async fn delete_session(&self, session_id: &str) -> Result<()>;

    // ========== 同步游标操作 ==========

    /// 保存同步游标
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `cursor`: 同步游标（格式：`seq:<seq>:<message_id>`）
    async fn save_sync_cursor(&self, session_id: &str, cursor: &SyncCursor) -> Result<()>;

    /// 获取同步游标
    async fn get_sync_cursor(&self, session_id: &str) -> Result<Option<SyncCursor>>;

    /// 获取所有会话的同步游标
    async fn get_all_sync_cursors(&self) -> Result<Vec<SyncCursor>>;

    // ========== 消息状态操作 ==========

    /// 保存消息状态（已读、已删除等）
    ///
    /// # 参数
    /// - `user_id`: 用户 ID
    /// - `message_id`: 消息 ID
    /// - `state`: 消息状态
    async fn save_message_state(
        &self,
        user_id: &str,
        message_id: &str,
        state: MessageState,
    ) -> Result<()>;

    /// 获取消息状态
    async fn get_message_state(
        &self,
        user_id: &str,
        message_id: &str,
    ) -> Result<Option<MessageState>>;

    /// 批量检查消息是否已删除（用于过滤已删除消息）
    ///
    /// # 参数
    /// - `user_id`: 用户 ID
    /// - `message_ids`: 消息 ID 列表
    ///
    /// # 返回
    /// - 已删除的消息 ID 集合
    async fn batch_check_deleted(
        &self,
        user_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<String>>;

    // ========== 扩展信息操作 ==========

    /// 保存消息扩展信息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    /// - `extension`: 消息扩展信息
    #[cfg(feature = "extensions")]
    async fn save_message_extension(
        &self,
        message_id: &str,
        extension: &MessageExtension,
    ) -> Result<()>;

    /// 获取消息扩展信息
    ///
    /// # 参数
    /// - `message_id`: 消息 ID
    ///
    /// # 返回
    /// - 消息扩展信息（如果存在）
    #[cfg(feature = "extensions")]
    async fn get_message_extension(&self, message_id: &str) -> Result<Option<MessageExtension>>;

    /// 保存会话扩展信息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `extension`: 会话扩展信息
    #[cfg(feature = "extensions")]
    async fn save_session_extension(
        &self,
        session_id: &str,
        extension: &SessionExtension,
    ) -> Result<()>;

    /// 获取会话扩展信息
    ///
    /// # 参数
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    /// - 会话扩展信息（如果存在）
    #[cfg(feature = "extensions")]
    async fn get_session_extension(&self, session_id: &str) -> Result<Option<SessionExtension>>;

    /// 批量获取消息扩展信息
    ///
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    ///
    /// # 返回
    /// - (消息ID, 扩展信息) 的列表
    #[cfg(feature = "extensions")]
    async fn batch_get_message_extensions(
        &self,
        message_ids: &[String],
    ) -> Result<Vec<(String, MessageExtension)>>;

    /// 批量获取会话扩展信息
    ///
    /// # 参数
    /// - `session_ids`: 会话 ID 列表
    ///
    /// # 返回
    /// - (会话ID, 扩展信息) 的列表
    #[cfg(feature = "extensions")]
    async fn batch_get_session_extensions(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<(String, SessionExtension)>>;
}

/// 会话过滤条件
#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    /// 会话类型过滤（可选）
    pub session_type: Option<String>,

    /// 业务类型过滤（可选）
    pub business_type: Option<String>,

    /// 是否只返回有未读消息的会话
    pub unread_only: bool,

    /// 最大返回数量
    pub limit: Option<usize>,

    /// 偏移量（用于分页）
    pub offset: Option<usize>,
}

impl SessionFilter {
    /// 创建默认过滤器
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置会话类型过滤
    pub fn with_session_type(mut self, session_type: String) -> Self {
        self.session_type = Some(session_type);
        self
    }

    /// 设置业务类型过滤
    pub fn with_business_type(mut self, business_type: String) -> Self {
        self.business_type = Some(business_type);
        self
    }

    /// 只返回有未读消息的会话
    pub fn unread_only(mut self) -> Self {
        self.unread_only = true;
        self
    }

    /// 设置最大返回数量
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// 设置偏移量
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = Some(offset);
        self
    }
}

/// 会话更新内容
#[derive(Debug, Clone)]
pub struct SessionUpdate {
    /// 更新最后消息信息
    pub last_message: Option<LastMessageUpdate>,

    /// 更新未读数
    pub unread_count: Option<i32>,

    /// 更新显示名称
    pub display_name: Option<String>,

    /// 更新元数据
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

impl SessionUpdate {
    /// 创建空的更新
    pub fn new() -> Self {
        Self {
            last_message: None,
            unread_count: None,
            display_name: None,
            metadata: None,
        }
    }

    /// 更新最后消息
    pub fn with_last_message(mut self, last_message: LastMessageUpdate) -> Self {
        self.last_message = Some(last_message);
        self
    }

    /// 更新未读数
    pub fn with_unread_count(mut self, unread_count: i32) -> Self {
        self.unread_count = Some(unread_count);
        self
    }

    /// 更新显示名称
    pub fn with_display_name(mut self, display_name: String) -> Self {
        self.display_name = Some(display_name);
        self
    }

    /// 更新元数据
    pub fn with_metadata(mut self, metadata: std::collections::HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// 最后消息更新信息
#[derive(Debug, Clone)]
pub struct LastMessageUpdate {
    /// 最后消息 ID
    pub message_id: String,

    /// 最后消息时间（毫秒时间戳）
    pub message_time: i64,

    /// 最后发送者 ID
    pub sender_id: Option<String>,

    /// 最后消息类型
    pub message_type: i32,

    /// 最后内容类型
    pub content_type: String,
}

/// 消息状态
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MessageState {
    /// 是否已读
    pub is_read: bool,

    /// 是否已删除（软删除）
    pub is_deleted: bool,

    /// 是否已销毁（阅后即焚）
    pub is_burned: bool,

    /// 已读时间（毫秒时间戳，可选）
    pub read_at: Option<i64>,

    /// 删除时间（毫秒时间戳，可选）
    pub deleted_at: Option<i64>,
}

impl MessageState {
    /// 创建默认状态（未读、未删除、未销毁）
    pub fn new() -> Self {
        Self {
            is_read: false,
            is_deleted: false,
            is_burned: false,
            read_at: None,
            deleted_at: None,
        }
    }

    /// 标记为已读
    pub fn mark_as_read(mut self) -> Self {
        self.is_read = true;
        self.read_at = Some(chrono::Utc::now().timestamp_millis());
        self
    }

    /// 标记为已删除
    pub fn mark_as_deleted(mut self) -> Self {
        self.is_deleted = true;
        self.deleted_at = Some(chrono::Utc::now().timestamp_millis());
        self
    }

    /// 标记为已销毁
    pub fn mark_as_burned(mut self) -> Self {
        self.is_burned = true;
        self
    }
}

impl Default for MessageState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_filter() {
        let filter = SessionFilter::new()
            .with_session_type("single".to_string())
            .unread_only()
            .with_limit(10);

        assert_eq!(filter.session_type, Some("single".to_string()));
        assert!(filter.unread_only);
        assert_eq!(filter.limit, Some(10));
    }

    #[test]
    fn test_session_update() {
        let update = SessionUpdate::new()
            .with_unread_count(5)
            .with_display_name("Test Session".to_string());

        assert_eq!(update.unread_count, Some(5));
        assert_eq!(update.display_name, Some("Test Session".to_string()));
    }

    #[test]
    fn test_message_state() {
        let state = MessageState::new().mark_as_read().mark_as_deleted();

        assert!(state.is_read);
        assert!(state.is_deleted);
        assert!(state.read_at.is_some());
        assert!(state.deleted_at.is_some());
    }
}
