//! IndexedDB 便捷接入
//!
//! 在 WASM 环境下，可通过实现 domain 层 Reader/Writer trait 接入 IndexedDB：
//!
//! 1. **实现 trait**：为 Message、Conversation、PendingSend、UserProfile 分别实现
//!    [MessageReader]/[MessageWriter]、[ConversationReader]/[ConversationWriter]、
//!    [PendingSendReader]/[PendingSendWriter]、[UserReader]/[UserWriter]。
//!
//! 2. **单类型适配器**：若已有「按 key 读写 blob」的 IndexedDB 封装，可用 [BackendAdapter]
//!    将 [MessageStorageBackend] 转为 [MessageReader] + [MessageWriter]（其他聚合同理）。
//!
//! 3. **示例**（伪代码）：
//!    ```ignore
//!    struct IdbMessageRepo { db: IdbDatabase, store: String }
//!    impl MessageReader for IdbMessageRepo { ... }
//!    impl MessageWriter for IdbMessageRepo { ... }
//!    // 注入到 StoreProvider 或应用层
//!    ```
//!
//! [MessageReader]: crate::domain::MessageReader
//! [MessageWriter]: crate::domain::MessageWriter
//! [ConversationReader]: crate::domain::ConversationReader
//! [ConversationWriter]: crate::domain::ConversationWriter
//! [PendingSendReader]: crate::domain::PendingSendReader
//! [PendingSendWriter]: crate::domain::PendingSendWriter
//! [UserReader]: crate::domain::UserReader
//! [UserWriter]: crate::domain::UserWriter

use async_trait::async_trait;
use prost::Message;

use crate::model::IMMessage;
use crate::shared::error::Result;

/// 可选：将「按 key 存 blob」的后端统一为 Message 的读/写。
/// 实现此 trait 后，可用 [MessageBackendAdapter] 得到 [MessageReader] + [MessageWriter]。
#[async_trait]
pub trait MessageStorageBackend: Send + Sync {
    async fn get_message(&self, message_id: &str) -> Result<Option<Vec<u8>>>;
    async fn put_message(&self, message_id: &str, data: &[u8]) -> Result<()>;
    async fn delete_message(&self, message_id: &str) -> Result<()>;
    async fn list_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<Vec<u8>>>;
}

/// 将 [MessageStorageBackend] 适配为 [MessageReader] + [MessageWriter]（需解码/编码 Message）
pub struct MessageBackendAdapter<B> {
    backend: B,
}

impl<B: MessageStorageBackend> MessageBackendAdapter<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }
}

// 如需实际使用，在此用 prost 解码/编码并 impl MessageReader + MessageWriter。
// 具体实现可放在 storage/indexeddb 或业务侧。
impl<B: MessageStorageBackend> MessageBackendAdapter<B> {
    /// 供 IndexedDB 实现方参考：从 backend 读 blob 并解码为 MessageVo
    pub async fn get_vo(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let data = self.backend.get_message(message_id).await?;
        let data = match data {
            Some(d) => d,
            None => return Ok(None),
        };
        let msg = Message::decode(data.as_slice()).map_err(|e| {
            crate::shared::error::FlareError::localized(
                crate::shared::error::ErrorCode::DatabaseError,
                e.to_string(),
            )
        })?;
        Ok(Some(IMMessage::new(msg)))
    }
}

// IndexedDB 接入：实现 domain 的 Reader/Writer，或 MessageStorageBackend + MessageBackendAdapter，
// 再将实现装入 StoreProvider 即可与 SQLite/Memory 路径一致使用。
