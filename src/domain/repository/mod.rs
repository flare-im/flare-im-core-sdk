//! 仓储接口定义（Port）
//!
//! Domain 层定义的仓储接口，由 Infrastructure 层实现
//!
//! 约束：
//! - Aggregate 可以使用 Repository
//! - Command Handler 可以使用 Repository
//! - Query Handler 禁止使用 Repository（必须使用 ReadStore）
//! - UI 禁止使用 Repository

use async_trait::async_trait;
use crate::domain::event::DomainEvent;
use crate::domain::message::Message;
use crate::domain::conversation::Conversation;

/// EventStore 接口
///
/// 用于存储领域事件，支持事件溯源
#[async_trait]
pub trait EventStore: Send + Sync {
    /// 追加事件
    async fn append(&self, event: DomainEvent) -> anyhow::Result<()>;
    
    /// 加载事件流
    async fn load_stream(&self, aggregate_id: &str) -> anyhow::Result<Vec<DomainEvent>>;
    
    /// 加载事件流（从指定版本开始）
    async fn load_stream_from_version(
        &self,
        aggregate_id: &str,
        from_version: u64,
    ) -> anyhow::Result<Vec<DomainEvent>>;
}

/// SnapshotStore 接口
///
/// 用于存储聚合根快照，加速恢复
#[async_trait::async_trait]
pub trait SnapshotStore<A>: Send + Sync
where
    A: Send + Sync + Clone,
{
    /// 加载快照
    async fn load(&self, aggregate_id: &str) -> anyhow::Result<Option<A>>;
    
    /// 保存快照
    async fn save(&self, aggregate_id: &str, aggregate: &A, version: u64) -> anyhow::Result<()>;
}

/// ReadStore 接口
///
/// 用于查询读模型，Query Handler 和 UI 使用
/// 
/// 设计原则（对标微信、Telegram、飞书）：
/// 1. **只读接口**：ReadStore 主要用于查询，写入通过 EventProjection 完成
/// 2. **优化查询**：支持索引、分页、游标等优化查询方式
/// 3. **最终一致性**：读模型通过事件投影异步更新，保证最终一致性
#[async_trait]
pub trait ReadStore: Send + Sync {
    /// 查询（通用查询接口）
    async fn query(&self, query: Query) -> anyhow::Result<QueryResult>;
    
    /// 写入消息到读模型（由 EventProjection 调用）
    ///
    /// 注意：这是内部接口，Command Handler 不应该直接调用
    /// 所有写入都应该通过 EventProjection 完成
    async fn write_message(&self, message: &Message) -> anyhow::Result<()>;
    
    /// 写入会话到读模型（由 EventProjection 调用）
    ///
    /// 注意：这是内部接口，Command Handler 不应该直接调用
    /// 所有写入都应该通过 EventProjection 完成
    async fn write_conversation(&self, conversation: &Conversation) -> anyhow::Result<()>;
    
    /// 更新会话（由 EventProjection 调用）
    async fn update_conversation(&self, conversation: &Conversation) -> anyhow::Result<()>;
    
    /// 删除消息（软删除，标记为已删除）
    async fn delete_message(&self, message_id: &str) -> anyhow::Result<()>;
    
    /// 删除会话中的所有消息
    async fn delete_conversation_messages(&self, conversation_id: &str) -> anyhow::Result<()>;
}

/// 查询类型
#[derive(Debug, Clone)]
pub enum Query {
    /// 查询会话列表
    ConversationList {
        limit: Option<usize>,
        cursor: Option<String>,
    },
    
    /// 查询会话详情
    ConversationDetail {
        conversation_id: String,
    },
    
    /// 查询消息列表
    MessageList {
        conversation_id: String,
        limit: Option<usize>,
        cursor: Option<String>,
    },
    
    /// 查询单条消息
    MessageDetail {
        message_id: String,
    },
    
    /// 搜索消息（按关键词）
    SearchMessages {
        conversation_id: Option<String>,
        keyword: String,
        limit: Option<usize>,
    },
    
    /// 查询消息（按类型和时间范围）
    FindMessages {
        conversation_id: Option<String>,
        message_type: Option<String>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<usize>,
    },
}

/// 查询结果
#[derive(Debug, Clone)]
pub enum QueryResult {
    /// 会话列表
    ConversationList {
        items: Vec<serde_json::Value>,
        next_cursor: Option<String>,
    },
    
    /// 会话详情
    ConversationDetail {
        item: serde_json::Value,
    },
    
    /// 消息列表
    MessageList {
        items: Vec<serde_json::Value>,
        next_cursor: Option<String>,
    },
    
    /// 消息详情
    MessageDetail {
        item: serde_json::Value,
    },
    
    /// 搜索结果
    SearchMessages {
        items: Vec<serde_json::Value>,
    },
    
    /// 查找结果
    FindMessages {
        items: Vec<serde_json::Value>,
    },
}
