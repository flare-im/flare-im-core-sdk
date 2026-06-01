use crate::model::message::MessageLocalState;
use crate::model::message::MessageStatus;
use crate::model::message::{IMMessage, SendAck};
use crate::util::date::prost_timestamp_to_ms;

pub const REASON_PENDING_ANOTHER_ACCOUNT: &str = "pending message belongs to another account";
pub const REASON_MAX_RETRIES_EXCEEDED: &str = "max retries exceeded";
pub const REASON_TIMEOUT_AFTER_RETRIES: &str = "timeout after retries";
pub const REASON_RECONCILED_FAILED: &str = "in-flight reconciled to failed by local terminal state";
pub const REASON_SEND_FAILED_BEFORE_ACK_MAX_RETRIES: &str =
    "send failed before ack; max retries exceeded";
pub const REASON_ORPHAN_RECOVERED: &str = "orphan sending message reconciled during queue recovery";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryLocalSnapshot {
    pub status: i32,
    pub server_id: String,
    pub seq: u64,
    pub conversation_id: String,
}

impl From<&IMMessage> for DeliveryLocalSnapshot {
    fn from(message: &IMMessage) -> Self {
        Self {
            status: message.status,
            server_id: message.server_id.clone(),
            seq: message.seq,
            conversation_id: message.conversation_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingDispatchDecision {
    DropAsCrossAccount { reason: &'static str },
    DropAsTerminal,
    FailMaxRetries { reason: &'static str },
    SendNow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryDecision {
    Retry { next_retry_count: u32 },
    Fail { reason: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InFlightReconcileDecision {
    KeepWaiting,
    MarkFailed { reason: &'static str },
    SynthesizeAck { snapshot: DeliveryLocalSnapshot },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingMessageConvergenceDecision {
    EmitReceived,
    MergePendingAndAck,
    DropDuplicate,
}

pub struct MessageDeliveryService;

impl MessageDeliveryService {
    fn apply_send_ack_state(message: &mut IMMessage) {
        message.local_state = MessageLocalState {
            sending: false,
            failed: false,
            is_local: false,
            sort_ts: message.local_state.sort_ts,
        };
        message.status = MessageStatus::Sent as i32;
        message.is_read = false;
    }

    pub fn decide_pending_dispatch(
        connected_user_id: &str,
        client_msg_id: &str,
        entry_sender_id: &str,
        local: Option<&DeliveryLocalSnapshot>,
        retries: u32,
        max_retries: u32,
    ) -> PendingDispatchDecision {
        if !connected_user_id.is_empty()
            && (entry_sender_id.trim().is_empty() || entry_sender_id != connected_user_id)
        {
            return PendingDispatchDecision::DropAsCrossAccount {
                reason: REASON_PENDING_ANOTHER_ACCOUNT,
            };
        }

        if let Some(local) = local {
            let already_final_success = local.status >= MessageStatus::Sent as i32
                && !local.server_id.trim().is_empty()
                && local.server_id != client_msg_id;
            let already_failed = local.status == MessageStatus::Failed as i32;
            if already_final_success || already_failed {
                return PendingDispatchDecision::DropAsTerminal;
            }
        }

        if retries >= max_retries {
            return PendingDispatchDecision::FailMaxRetries {
                reason: REASON_MAX_RETRIES_EXCEEDED,
            };
        }

        PendingDispatchDecision::SendNow
    }

    pub fn decide_send_attempt_failure(retries: u32, max_retries: u32) -> RetryDecision {
        let next_retry_count = retries.saturating_add(1);
        if next_retry_count >= max_retries {
            RetryDecision::Fail {
                reason: REASON_SEND_FAILED_BEFORE_ACK_MAX_RETRIES,
            }
        } else {
            RetryDecision::Retry { next_retry_count }
        }
    }

    pub fn decide_timeout_expiry(retries: u32, max_retries: u32) -> RetryDecision {
        if retries >= max_retries {
            RetryDecision::Fail {
                reason: REASON_TIMEOUT_AFTER_RETRIES,
            }
        } else {
            RetryDecision::Retry {
                next_retry_count: retries.saturating_add(1),
            }
        }
    }

    pub fn reconcile_in_flight(
        local: Option<&DeliveryLocalSnapshot>,
        client_msg_id: &str,
    ) -> InFlightReconcileDecision {
        let Some(local) = local else {
            return InFlightReconcileDecision::KeepWaiting;
        };

        if local.status == MessageStatus::Failed as i32 {
            return InFlightReconcileDecision::MarkFailed {
                reason: REASON_RECONCILED_FAILED,
            };
        }

        let local_sent_like = local.status >= MessageStatus::Sent as i32
            && !local.server_id.trim().is_empty()
            && local.server_id != client_msg_id;
        if local_sent_like {
            return InFlightReconcileDecision::SynthesizeAck {
                snapshot: local.clone(),
            };
        }

        InFlightReconcileDecision::KeepWaiting
    }

    pub fn decide_incoming_message_convergence(
        current_user_id: &str,
        incoming: &IMMessage,
        local_by_client: Option<&IMMessage>,
        local_by_server: Option<&IMMessage>,
    ) -> IncomingMessageConvergenceDecision {
        if local_by_server.is_some() {
            return IncomingMessageConvergenceDecision::DropDuplicate;
        }

        if current_user_id.is_empty()
            || incoming.client_msg_id.trim().is_empty()
            || incoming.sender_id.trim().is_empty()
            || incoming.sender_id != current_user_id
        {
            return IncomingMessageConvergenceDecision::EmitReceived;
        }

        let Some(local) = local_by_client else {
            return IncomingMessageConvergenceDecision::EmitReceived;
        };
        let local_pending_like = local.server_id == local.client_msg_id
            || local.local_state.is_local
            || local.local_state.sending
            || local.status < MessageStatus::Sent as i32;
        if local_pending_like {
            IncomingMessageConvergenceDecision::MergePendingAndAck
        } else {
            IncomingMessageConvergenceDecision::EmitReceived
        }
    }

    pub fn mark_failed(message: &IMMessage) -> IMMessage {
        let mut failed_message = message.clone();
        failed_message.server_id = failed_message.client_msg_id.clone();
        failed_message.local_state = MessageLocalState {
            sending: false,
            failed: true,
            is_local: true,
            sort_ts: failed_message.local_state.sort_ts,
        };
        failed_message.status = MessageStatus::Failed as i32;
        failed_message
    }

    pub fn mark_sent_from_ack(message: &IMMessage, ack: &SendAck) -> IMMessage {
        let mut sent_message = message.clone();
        if !ack.server_msg_id.is_empty() {
            sent_message.server_id = ack.server_msg_id.clone();
        }
        sent_message.seq = ack.seq;
        // 与下行消息时间轴对齐；否则 ACK 后仍持客户端建消息时间，`sort_ts`/首屏排序可能弱于服务端历史而掉出 LIMIT。
        let server_ms = prost_timestamp_to_ms(ack.server_time.as_ref());
        if server_ms > 0 {
            sent_message.timestamp = server_ms;
            sent_message.client_timestamp = server_ms;
        }
        Self::apply_send_ack_state(&mut sent_message);
        sent_message
    }

    pub fn merge_incoming_as_sent(local: Option<&IMMessage>, incoming: &IMMessage) -> IMMessage {
        let mut merged = incoming.clone();
        merged.local_state = MessageLocalState {
            sending: false,
            failed: false,
            is_local: false,
            sort_ts: local
                .map(|message| message.local_state.sort_ts)
                .unwrap_or(incoming.local_state.sort_ts.max(incoming.timestamp)),
        };
        Self::apply_send_ack_state(&mut merged);
        merged
    }

    pub fn sanitize_send_ack_message(message: &IMMessage) -> IMMessage {
        let mut sanitized = message.clone();
        Self::apply_send_ack_state(&mut sanitized);
        sanitized
    }

    pub fn synthetic_ack(client_msg_id: &str, snapshot: &DeliveryLocalSnapshot) -> SendAck {
        SendAck {
            client_msg_id: client_msg_id.to_string(),
            server_msg_id: snapshot.server_id.clone(),
            seq: snapshot.seq,
            conversation_id: snapshot.conversation_id.clone(),
            success: true,
            ..Default::default()
        }
    }

    pub fn synthetic_ack_from_incoming(incoming: &IMMessage) -> SendAck {
        SendAck {
            client_msg_id: incoming.client_msg_id.clone(),
            server_msg_id: incoming.server_id.clone(),
            seq: incoming.seq,
            conversation_id: incoming.conversation_id.clone(),
            success: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DeliveryLocalSnapshot, InFlightReconcileDecision, IncomingMessageConvergenceDecision,
        MessageDeliveryService, PendingDispatchDecision, RetryDecision,
    };
    use crate::model::message::{IMMessage, MessageStatus};

    #[test]
    fn pending_dispatch_drops_cross_account_entry() {
        let decision =
            MessageDeliveryService::decide_pending_dispatch("u1", "c1", "u2", None, 0, 3);

        assert!(matches!(
            decision,
            PendingDispatchDecision::DropAsCrossAccount { .. }
        ));
    }

    #[test]
    fn pending_dispatch_drops_terminal_local_entry() {
        let local = DeliveryLocalSnapshot {
            status: MessageStatus::Sent as i32,
            server_id: "s1".to_string(),
            seq: 1,
            conversation_id: "c1".to_string(),
        };
        let decision =
            MessageDeliveryService::decide_pending_dispatch("u1", "c1", "u1", Some(&local), 0, 3);

        assert_eq!(decision, PendingDispatchDecision::DropAsTerminal);
    }

    #[test]
    fn timeout_decision_retries_before_max() {
        let decision = MessageDeliveryService::decide_timeout_expiry(1, 3);

        assert_eq!(
            decision,
            RetryDecision::Retry {
                next_retry_count: 2
            }
        );
    }

    #[test]
    fn reconcile_in_flight_synthesizes_ack_for_sent_like_local() {
        let local = DeliveryLocalSnapshot {
            status: MessageStatus::Sent as i32,
            server_id: "server-1".to_string(),
            seq: 9,
            conversation_id: "c1".to_string(),
        };

        let decision = MessageDeliveryService::reconcile_in_flight(Some(&local), "client-1");

        match decision {
            InFlightReconcileDecision::SynthesizeAck { snapshot } => {
                assert_eq!(snapshot.server_id, "server-1");
                assert_eq!(snapshot.seq, 9);
            }
            _ => panic!("expected synthetic ack decision"),
        }
    }

    #[test]
    fn incoming_duplicate_by_server_id_is_dropped() {
        let mut incoming = IMMessage::new(flare_proto::common::Message::default());
        incoming.server_id = "server-1".to_string();
        incoming.sender_id = "u2".to_string();

        let decision = MessageDeliveryService::decide_incoming_message_convergence(
            "u1",
            &incoming,
            None,
            Some(&incoming),
        );

        assert_eq!(decision, IncomingMessageConvergenceDecision::DropDuplicate);
    }

    #[test]
    fn incoming_self_echo_is_send_ack_not_read_receipt() {
        let mut local = IMMessage::new(flare_proto::common::Message::default());
        local.client_msg_id = "client-1".to_string();
        local.server_id = "client-1".to_string();
        local.sender_id = "u1".to_string();
        local.status = MessageStatus::Created as i32;
        local.is_read = false;
        local.local_state.sending = true;
        local.local_state.is_local = true;
        local.local_state.sort_ts = 42;

        let mut incoming = IMMessage::new(flare_proto::common::Message::default());
        incoming.client_msg_id = "client-1".to_string();
        incoming.server_id = "server-1".to_string();
        incoming.sender_id = "u1".to_string();
        incoming.seq = 7;
        incoming.status = MessageStatus::Read as i32;
        incoming.is_read = true;

        let merged = MessageDeliveryService::merge_incoming_as_sent(Some(&local), &incoming);

        assert_eq!(merged.server_id, "server-1");
        assert_eq!(merged.seq, 7);
        assert_eq!(merged.status, MessageStatus::Sent as i32);
        assert!(!merged.is_read);
        assert!(!merged.local_state.sending);
        assert!(!merged.local_state.is_local);
        assert_eq!(merged.local_state.sort_ts, 42);
    }

    #[test]
    fn send_ack_sanitizer_never_preserves_read_state() {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.status = MessageStatus::Read as i32;
        message.is_read = true;
        message.local_state.sending = true;
        message.local_state.is_local = true;

        let sanitized = MessageDeliveryService::sanitize_send_ack_message(&message);

        assert_eq!(sanitized.status, MessageStatus::Sent as i32);
        assert!(!sanitized.is_read);
        assert!(!sanitized.local_state.sending);
        assert!(!sanitized.local_state.is_local);
    }
}
