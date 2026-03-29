use async_trait::async_trait;

use crate::error::Result;
use crate::model::IMMessage;

/// 消息存储 trait — 可插拔后端（SQLite / IndexedDB / Memory）
/// 仅存 content_bytes，不存 content；文本消息可额外存 text 列供搜索。
#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn save_batch(&self, messages: &[IMMessage]) -> Result<()>;
    async fn get(&self, message_id: &str) -> Result<Option<IMMessage>>;
    async fn get_by_client_msg_id(&self, client_msg_id: &str) -> Result<Option<IMMessage>> {
        self.get(client_msg_id).await
    }
    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>>;
    async fn update_status(&self, message_id: &str, status: i32) -> Result<()>;
    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()>;
    async fn delete(&self, message_id: &str) -> Result<()>;
    async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>>;

    /// 发送 ACK 后更新：删除乐观消息行（server_id=client_msg_id），再写入终态消息（server_id=server_msg_id），原子化
    async fn update_after_ack(&self, client_msg_id: &str, message: &IMMessage) -> Result<()>;
}
