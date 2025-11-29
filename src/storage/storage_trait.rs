//! 存储抽象层
//!
//! 定义统一的存储接口，支持不同平台的存储实现（SQLite、IndexedDB）

use crate::model::{
    Message,
    SessionSummary,
    SyncCursor,
    MessageExtension, SessionExtension,
};
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
    
    /// 根据消息 ID 获取消息
    async fn get_message(&self, message_id: &str) -> Result<Option<Message>>;
    
    /// 批量获取消息（优化：减少数据库往返）
    /// 
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    /// 
    /// # 返回
    /// - 消息列表（按 message_ids 顺序，不存在的消息会被跳过）
    async fn batch_get_messages(
        &self,
        message_ids: &[String],
    ) -> Result<Vec<Message>> {
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
    async fn batch_get_sessions(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<SessionSummary>> {
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
    async fn update_session(
        &self,
        session_id: &str,
        updates: SessionUpdate,
    ) -> Result<()>;
    
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
    async fn get_message_extension(
        &self,
        message_id: &str,
    ) -> Result<Option<MessageExtension>>;
    
    /// 保存会话扩展信息
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `extension`: 会话扩展信息
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
    async fn get_session_extension(
        &self,
        session_id: &str,
    ) -> Result<Option<SessionExtension>>;
    
    /// 批量获取消息扩展信息
    /// 
    /// # 参数
    /// - `message_ids`: 消息 ID 列表
    /// 
    /// # 返回
    /// - (消息ID, 扩展信息) 的列表
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
        let state = MessageState::new()
            .mark_as_read()
            .mark_as_deleted();
        
        assert!(state.is_read);
        assert!(state.is_deleted);
        assert!(state.read_at.is_some());
        assert!(state.deleted_at.is_some());
    }
}
