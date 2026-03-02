//! 会话仓储接口
//!
//! 用于会话聚合根的存储和查询。
//! 会话具有低频写入、关系查询的特点，建议使用关系数据库（如 PostgreSQL）。

use async_trait::async_trait;
use crate::domain::conversation::Conversation;

/// 会话列表查询结果
#[derive(Debug, Clone)]
pub struct ConversationListResult {
    /// 会话列表
    pub conversations: Vec<Conversation>,
    /// 下一页游标（None 表示没有更多数据）
    pub next_cursor: Option<String>,
}

/// 会话仓储接口
///
/// 用于会话聚合根的存储和查询。
/// 会话具有低频写入、关系查询的特点，建议使用关系数据库（如 PostgreSQL）。
///
/// ## 实现要求
///
/// - 必须支持事务（保证数据一致性）
/// - 建议实现索引以支持快速查询（conversation_id, user_id）
/// - 建议实现缓存以提高查询性能
///
/// ## 使用示例
///
/// ```no_run
/// use async_trait::async_trait;
/// use flare_im_core_sdk::domain::repository::ConversationRepository;
/// use flare_im_core_sdk::domain::conversation::Conversation;
/// use std::sync::Arc;
///
/// struct MyConversationRepository { /* ... */ }
///
/// #[async_trait]
/// impl ConversationRepository for MyConversationRepository {
///     async fn save(&self, conversation: &Conversation) -> anyhow::Result<()> {
///         // 实现存储逻辑
///         Ok(())
///     }
///
///     async fn update(&self, conversation: &Conversation) -> anyhow::Result<()> {
///         // 实现更新逻辑
///         Ok(())
///     }
///
///     // ... 其他方法
/// }
/// ```
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    /// 保存会话
    ///
    /// # 参数
    /// * `conversation` - 要存储的会话
    ///
    /// # 返回
    /// * `Ok(())` - 存储成功
    /// * `Err` - 存储失败
    async fn save(&self, conversation: &Conversation) -> anyhow::Result<()>;
    
    /// 更新会话
    ///
    /// # 参数
    /// * `conversation` - 要更新的会话
    ///
    /// # 返回
    /// * `Ok(())` - 更新成功
    /// * `Err` - 更新失败
    async fn update(&self, conversation: &Conversation) -> anyhow::Result<()>;
    
    /// 根据 ID 查找会话
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID
    ///
    /// # 返回
    /// * `Ok(Some(Conversation))` - 找到会话
    /// * `Ok(None)` - 未找到会话
    /// * `Err` - 查询失败
    async fn find_by_id(&self, conversation_id: &str) -> anyhow::Result<Option<Conversation>>;
    
    /// 查找所有会话（分页）
    ///
    /// # 参数
    /// * `limit` - 限制数量
    /// * `cursor` - 游标（用于分页）
    ///
    /// # 返回
    /// * `Ok(ConversationListResult)` - 会话列表和下一页游标
    /// * `Err` - 查询失败
    async fn find_all(
        &self,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> anyhow::Result<ConversationListResult>;
    
    /// 根据参与者查找会话
    ///
    /// # 参数
    /// * `user_id` - 用户 ID
    /// * `limit` - 限制数量
    ///
    /// # 返回
    /// * `Ok(Vec<Conversation>)` - 会话列表
    /// * `Err` - 查询失败
    async fn find_by_participant(
        &self,
        user_id: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Conversation>>;
    
    /// 删除会话
    ///
    /// # 参数
    /// * `conversation_id` - 会话 ID
    ///
    /// # 返回
    /// * `Ok(())` - 删除成功
    /// * `Err` - 删除失败
    async fn delete(&self, conversation_id: &str) -> anyhow::Result<()>;
}
