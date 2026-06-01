use async_trait::async_trait;

use crate::error::Result;
use crate::model::ConversationParticipant;

#[async_trait]
pub trait ConversationParticipantStore: Send + Sync {
    async fn save_page(
        &self,
        conversation_id: &str,
        participants: &[ConversationParticipant],
        participant_version: u64,
        replace_all: bool,
    ) -> Result<()>;

    async fn list(
        &self,
        conversation_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<ConversationParticipant>>;

    async fn version(&self, conversation_id: &str) -> Result<u64>;

    /// 用户资料变更后，批量刷新本地参与者快照中的昵称/头像。
    async fn patch_user_display(
        &self,
        user_id: &str,
        nickname: &str,
        avatar_url: &str,
    ) -> Result<()> {
        let _ = (user_id, nickname, avatar_url);
        Ok(())
    }
}
