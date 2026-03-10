use crate::model::conversation::*;
use crate::model::message::ConversationUpdateEvent;

/// 会话相关事件
#[derive(Clone, Debug)]
pub enum ConversationEvent {
    /// 全量会话同步完成
    Synced { conversations: Vec<ConversationSummary> },
    /// 增量会话更新（ConversationPatch，由同步流程产生）
    Patched { patch: ConversationPatch },
    /// 会话属性变更（EVENT_CONVERSATION_UPDATE 推送）
    Updated { conversation_id: String, event: ConversationUpdateEvent },
    /// 会话已删除
    Deleted { conversation_id: String },
}
