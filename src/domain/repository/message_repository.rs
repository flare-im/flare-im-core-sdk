use async_trait::async_trait;

use crate::error::Result;
use crate::model::IMMessage;

/// 消息查询（只读）
#[async_trait]
pub trait MessageReader: Send + Sync {
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>>;
    /// 按 client_msg_id 查询（发送中/待 ACK 时可能仅有 client_msg_id）
    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>>;
    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>>;
    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>>;
}

/// 消息写操作
#[async_trait]
pub trait MessageWriter: Send + Sync {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()>;
    async fn save_one(&self, message: &IMMessage) -> Result<()>;
    async fn update_status(&self, message_id: &str, status: i32) -> Result<()>;
    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()>;
    async fn delete(&self, message_id: &str) -> Result<()>;

    /// 发送 ACK 后更新：删除以 client_msg_id 为 server_id 的乐观写入行，再写入带 server_msg_id/seq 的终态消息（原子化，保证主键从 client_msg_id 迁移到 server_msg_id）
    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()>;
}
