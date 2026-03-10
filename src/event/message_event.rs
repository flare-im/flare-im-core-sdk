use crate::model::message::*;

/// 消息相关事件
#[derive(Clone, Debug)]
pub enum MessageEvent {
    /// 收到新消息
    Received { message: Message },
    /// 消息被撤回
    Recalled { conversation_id: String, event: MessageRecallEvent },
    /// 消息被编辑
    Edited { conversation_id: String, event: MessageEditEvent },
    /// 消息被删除
    Deleted { conversation_id: String, event: MessageDeleteEvent },
    /// 已读回执
    ReadReceipt { conversation_id: String, event: ReadReceiptEvent },
    /// 表情反应变更
    ReactionUpdated { conversation_id: String, event: ReactionEvent },
    /// 消息置顶
    Pinned { conversation_id: String, event: PinEvent },
    /// 取消消息置顶
    Unpinned { conversation_id: String, event: UnpinEvent },
    /// 消息标记
    Marked { conversation_id: String, event: MarkEvent },
    /// 取消消息标记
    Unmarked { conversation_id: String, event: UnmarkEvent },
    /// 正在输入
    Typing { conversation_id: String, event: TypingEvent },
    /// 发送消息回执
    SendAck { ack: SendAck },
}
