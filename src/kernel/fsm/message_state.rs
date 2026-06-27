//! 消息生命周期 FSM（Lifecycle Pattern）
//!
//! 状态：Pending → Sending → Sent → Delivered → Read；及 Failed / Recalled / Deleted。
//! 供 UI 展示「✔ / ✔✔ / 已读」等；转移由 SendAck、已读回执等事件驱动。

use crate::shared::error::{FlareError, Result};

/// 消息状态（与 proto MessageStatus 对齐，并增加 Pending/Sending 本地态）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageState {
    /// 已入队待发送（可靠队列内）
    Pending,
    /// 正在发送（已发出，等待 ack）
    Sending,
    /// 已发送（收到 SendAck）
    Sent,
    /// 已送达
    Delivered,
    /// 已读
    Read,
    /// 发送失败（超时或重试耗尽）
    Failed,
    /// 已撤回
    Recalled,
    /// 已删除
    Deleted,
}

/// 触发消息状态转移的事件
#[derive(Clone, Debug)]
pub enum MessageStateEvent {
    /// 提交发送（入队）
    Enqueued,
    /// 开始发送（从队列取出并发出）
    SendStarted,
    /// 收到 SendAck
    SendAckReceived,
    /// 送达回执
    Delivered,
    /// 已读回执
    Read,
    /// 发送超时或失败
    SendFailed,
    /// 撤回
    Recalled,
    /// 删除
    Deleted,
}

/// 消息状态 FSM
pub struct MessageStateFsm;

impl MessageStateFsm {
    pub fn transition(from: MessageState, event: &MessageStateEvent) -> Result<MessageState> {
        use MessageState as S;
        use MessageStateEvent as E;

        let next = match (from, event) {
            (S::Pending, E::SendStarted) => S::Sending,
            (S::Sending, E::SendAckReceived) => S::Sent,
            (S::Sending, E::SendFailed) => S::Failed,
            (S::Sent, E::Delivered) => S::Delivered,
            (S::Sent, E::Read) => S::Read,
            (S::Delivered, E::Read) => S::Read,
            (S::Sent, E::Recalled) | (S::Delivered, E::Recalled) | (S::Read, E::Recalled) => {
                S::Recalled
            }
            (S::Sent, E::Deleted)
            | (S::Delivered, E::Deleted)
            | (S::Read, E::Deleted)
            | (S::Recalled, E::Deleted) => S::Deleted,
            (_, E::Enqueued) if from == S::Pending => S::Pending,
            _ => {
                return Err(FlareError::system(format!(
                    "invalid message state transition: {:?} + {:?}",
                    from, event
                )));
            }
        };
        Ok(next)
    }

    /// 是否为终态（不再转移）
    pub fn is_terminal(s: MessageState) -> bool {
        matches!(
            s,
            MessageState::Failed | MessageState::Recalled | MessageState::Deleted
        )
    }

    /// 从 MessageLocalState 推断当前展示用状态（用于本地持久化与 UI）
    pub fn from_local_state(sending: bool, failed: bool, is_local: bool) -> MessageState {
        if failed {
            MessageState::Failed
        } else if sending {
            MessageState::Sending
        } else if is_local {
            MessageState::Pending
        } else {
            MessageState::Sent
        }
    }

    /// 将 FSM 状态转为本地持久化标志 (sending, failed, is_local)
    pub fn to_local_state_flags(s: MessageState) -> (bool, bool, bool) {
        match s {
            MessageState::Pending => (false, false, true),
            MessageState::Sending => (true, false, true),
            MessageState::Sent | MessageState::Delivered | MessageState::Read => {
                (false, false, false)
            }
            MessageState::Failed => (false, true, true),
            MessageState::Recalled | MessageState::Deleted => (false, false, false),
        }
    }
}
