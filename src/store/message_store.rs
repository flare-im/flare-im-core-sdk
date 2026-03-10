use async_trait::async_trait;
use crate::error::Result;
use crate::model::message::Message;

/// 消息存储 trait — 可插拔后端（SQLite / IndexedDB / Memory）
///
/// 覆盖全部消息操作的本地持久化需求：
/// - 新消息、批量保存
/// - 状态更新（撤回/删除）
/// - 内容更新（编辑）
/// - 查询、搜索
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// 批量保存消息（新消息/同步消息）
    async fn save_batch(&self, messages: &[Message]) -> Result<()>;

    /// 获取单条消息
    async fn get(&self, message_id: &str) -> Result<Option<Message>>;

    /// 按会话查询消息（before_seq 倒序分页）
    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<Message>>;

    /// 更新消息状态（撤回/删除等 MessageStatus 变更）
    async fn update_status(&self, message_id: &str, status: i32) -> Result<()>;

    /// 更新消息内容（编辑操作：new_content bytes + 可选版本号）
    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()>;

    /// 删除消息
    async fn delete(&self, message_id: &str) -> Result<()>;

    /// 全文搜索
    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<Message>>;
}
