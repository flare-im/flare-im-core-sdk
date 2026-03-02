//! 消息仓储接口
//!
//! 用于消息聚合根的存储和查询。
//! 消息具有高频写入、时序查询的特点，建议使用时序数据库（如 TimescaleDB）。

use async_trait::async_trait;
use crate::domain::message::Message;

/// 消息列表查询结果
#[derive(Debug, Clone)]
pub struct MessageListResult {
    /// 消息列表
    pub messages: Vec<Message>,
    /// 下一页游标（None 表示没有更多数据）
    pub next_cursor: Option<String>,
}

/// 消息仓储接口
///
/// 用于消息聚合根的存储和查询。
/// 消息具有高频写入、时序查询的特点，建议使用时序数据库（如 TimescaleDB）。
///
/// ## 实现要求
///
/// - 必须支持事务（保证数据一致性）
/// - 建议实现索引以支持快速查询（conversation_id, message_id, timestamp）
/// - 建议实现批量写入以提高性能
/// - 建议实现分页查询（使用 cursor）
///
/// ## 使用示例
///
/// ```no_run
/// use async_trait::async_trait;
/// use flare_im_core_sdk::domain::repository::MessageRepository;
/// use flare_im_core_sdk::domain::message::Message;
/// use std::sync::Arc;
///
/// struct MyMessageRepository { /* ... */ }
///
/// #[async_trait]
/// impl MessageRepository for MyMessageRepository {
///     async fn save(&self, message: &Message) -> anyhow::Result<()> {
///         // 实现存储逻辑
///         Ok(())
///     }
///
///     async fn save_batch(&self, messages: &[Message]) -> anyhow::Result<()> {
///         // 实现批量存储逻辑
///         Ok(())
///     }
///
///     // ... 其他方法
/// }
/// ```
#[async_trait]
pub trait MessageRepository: Send + Sync {
    /// 保存消息
    ///
    /// # 参数
    /// * `message` - 要存储的消息
    ///
    /// # 返回
    /// * `Ok(())` - 存储成功
    /// * `Err` - 存储失败
    async fn save(&self, message: &Message) -> anyhow::Result<()>;
    
    /// 批量保存消息
    ///
    /// # 参数
    /// * `messages` - 要存储的消息列表
    ///
    /// # 返回
    /// * `Ok(())` - 存储成功
    /// * `Err` - 存储失败
    ///
    /// # 注意
    /// 实现应该使用事务保证原子性
    async fn save_batch(&self, messages: &[Message]) -> anyhow::Result<()>;
    
    /// 根据 ID 查找消息
    ///
    /// # 参数
    /// * `message_id` - 消息 ID（server_id 或 client_msg_id）
    ///
    /// # 返回
    /// * `Ok(Some(Message))` - 找到消息
    /// * `Ok(None)` - 未找到消息
    /// * `Err` - 查询失败
    async fn find_by_id(&self, message_id: &str) -> anyhow::Result<Option<Message>>;
    
    /// 根据会话 ID 查找消息列表
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID
    /// * `limit` - 限制数量
    /// * `cursor` - 游标（用于分页）
    ///
    /// # 返回
    /// * `Ok(MessageListResult)` - 消息列表和下一页游标
    /// * `Err` - 查询失败
    async fn find_by_conversation(
        &self,
        conversation_id: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> anyhow::Result<MessageListResult>;
    
    /// 搜索消息（按关键词）
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID（可选，None 表示搜索所有会话）
    /// * `keyword` - 搜索关键词
    /// * `limit` - 限制数量
    ///
    /// # 返回
    /// * `Ok(Vec<Message>)` - 匹配的消息列表
    /// * `Err` - 查询失败
    async fn search(
        &self,
        conversation_id: Option<&str>,
        keyword: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Message>>;
    
    /// 根据时间范围查找消息
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID（可选）
    /// * `start_time` - 开始时间（可选）
    /// * `end_time` - 结束时间（可选）
    /// * `limit` - 限制数量
    ///
    /// # 返回
    /// * `Ok(Vec<Message>)` - 匹配的消息列表
    /// * `Err` - 查询失败
    async fn find_by_time_range(
        &self,
        conversation_id: Option<&str>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Message>>;
    
    /// 删除消息
    ///
    /// # 参数
    /// * `message_id` - 消息 ID
    ///
    /// # 返回
    /// * `Ok(())` - 删除成功
    /// * `Err` - 删除失败
    async fn delete(&self, message_id: &str) -> anyhow::Result<()>;
    
    /// 删除会话的所有消息
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID
    ///
    /// # 返回
    /// * `Ok(())` - 删除成功
    /// * `Err` - 删除失败
    async fn delete_by_conversation(&self, conversation_id: &str) -> anyhow::Result<()>;
}
