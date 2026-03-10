use std::sync::Arc;

use crate::conversation;
use crate::core::CurrentUserIdStore;
use crate::error::{SdkError, Result};
use crate::model::conversation::ConversationSummary;
use crate::query::{GetConversationsQuery, GetConversationQuery};
use crate::store::ConversationStore;

/// 会话 API
///
/// 提供会话列表/详情/已读/删除，以及**创建会话 ID**（与 flare-core 规则一致，多端确定性）。
///
/// ```ignore
/// let conversations = client.conversation().list().await?;
/// client.conversation().mark_read("conv_id", 100).await?;
/// let cid = client.conversation().single_chat_id_for_current_user("peer_id").await?;
/// ```
pub struct ConversationApi {
    store: Arc<dyn ConversationStore>,
    current_user_id: CurrentUserIdStore,
}

impl ConversationApi {
    pub fn new(store: Arc<dyn ConversationStore>, current_user_id: CurrentUserIdStore) -> Self {
        Self { store, current_user_id }
    }

    /// 从 SDK 获取当前用户 ID（连接后有效，未连接返回 NotConnected）
    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(SdkError::NotConnected);
        }
        Ok(uid)
    }

    /// 单聊会话 ID（当前用户 + 对方 user_id；当前用户从 SDK 获取）
    pub async fn single_chat_id_for_current_user(&self, peer_id: &str) -> Result<String> {
        let uid = self.current_user_id().await?;
        Ok(self.single_chat_id(&uid, peer_id))
    }

    // ── 会话 ID 生成（统一走 flare-core 规则，与 chatroom_client / 服务端一致）────────────

    /// 单聊会话 ID（CID 格式：1A + OpaqueID，双方 user_id 排序后确定性生成）
    pub fn single_chat_id(&self, user1: &str, user2: &str) -> String {
        conversation::generate_single_chat_conversation_id(user1, user2)
    }

    /// 群聊会话 ID（CID 格式：2A + OpaqueID）
    pub fn group_id(&self, group_id: &str) -> String {
        conversation::generate_group_conversation_id(group_id)
    }

    /// AI 助手会话 ID（CID 格式：3A + OpaqueID）
    pub fn ai_id(&self, user_id: &str, ai_scope: &str) -> String {
        conversation::generate_ai_conversation_id(user_id, ai_scope)
    }

    /// 客服会话 ID（CID 格式：5A + OpaqueID）
    pub fn customer_id(&self, customer_id: &str, channel: &str) -> String {
        conversation::generate_customer_conversation_id(customer_id, channel)
    }

    /// 系统通知会话 ID（CID 格式：4A + OpaqueID）
    pub fn system_id(&self, system_id: &str, scope: Option<&str>) -> String {
        conversation::generate_system_conversation_id(
            system_id,
            scope.map(std::string::ToString::to_string),
        )
    }

    /// 临时会话 ID（非确定性，6A + ULID）
    pub fn temp_id(&self) -> String {
        conversation::generate_temp_conversation_id()
    }

    // ── 存储与查询 ────────────────────────────────────────────────────────────────────

    pub async fn list(&self) -> Result<Vec<ConversationSummary>> {
        GetConversationsQuery.execute(self.store.as_ref()).await
    }

    pub async fn get(&self, conversation_id: &str) -> Result<Option<ConversationSummary>> {
        GetConversationQuery { conversation_id: conversation_id.into() }
            .execute(self.store.as_ref()).await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.store.update_unread(conversation_id, 0, read_seq).await
    }

    /// 全部已读：拉取会话列表后对每条会话执行 mark_read(conversation_id, max_seq)
    pub async fn mark_all_read(&self) -> Result<()> {
        let list = self.list().await?;
        for c in list {
            let _ = self.mark_read(&c.conversation_id, c.max_seq).await;
        }
        Ok(())
    }

    pub async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.store.delete(conversation_id).await
    }

    /// 在本地会话列表中确保存在该会话（用于「创建会话」后立即可见）。
    /// 若本地已有则跳过；否则写入一条最小摘要。单聊时可传 peer_id 写入 ext，供发送消息时填 receiver_id。
    pub async fn ensure_local_conversation(
        &self,
        conversation_id: &str,
        display_name: Option<&str>,
        conversation_type: &str,
        business_type: &str,
        peer_id: Option<&str>,
    ) -> Result<()> {
        if self.get(conversation_id).await?.is_some() {
            return Ok(());
        }
        let mut ext = std::collections::HashMap::new();
        if let Some(pid) = peer_id {
            ext.insert("peer_id".to_string(), pid.to_string());
        }
        let summary = ConversationSummary {
            conversation_id: conversation_id.to_string(),
            conversation_type: conversation_type.to_string(),
            business_type: business_type.to_string(),
            display_name: display_name.unwrap_or("").to_string(),
            ext,
            ..Default::default()
        };
        self.store.save_batch(&[summary]).await
    }
}
