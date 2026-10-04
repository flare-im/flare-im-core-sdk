//! Example assertions: queue admission is not a persisted server ACK.
use flare_im_core_sdk::model::message::{SendAck, SendAckDurability, send_ack};
use flare_im_core_sdk::prelude::*;
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

pub struct SendConfirmation {
    _subscription: Subscription,
    receiver: oneshot::Receiver<io::Result<SendAck>>,
}

impl SendConfirmation {
    // Subscribe before send, including for direct-send clients: no ACK race.
    pub fn observe(client: &IMClient, client_msg_id: &str) -> Result<Self> {
        let id = client_msg_id.to_owned();
        let (sender, receiver) = oneshot::channel();
        let sender = Arc::new(Mutex::new(Some(sender)));
        let subscription = client.on_any(move |event| {
            if let Some(result) = terminal_result(event.as_ref(), &id)
                && let Ok(mut sender) = sender.lock()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(result);
            }
        })?;
        Ok(Self {
            _subscription: subscription,
            receiver,
        })
    }

    pub async fn wait(self) -> io::Result<SendAck> {
        tokio::time::timeout(Duration::from_secs(30), self.receiver)
            .await
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "persisted server ACK not received within 30s",
                )
            })?
            .map_err(|_| io::Error::other("send confirmation subscription closed"))?
    }
}

fn terminal_result(event: &SdkEvent, id: &str) -> Option<io::Result<SendAck>> {
    match event {
        SdkEvent::Message(MessageEvent::SendAck { ack }) if ack.client_msg_id == id => {
            match ack.result.as_ref() {
                Some(send_ack::Result::Accepted(accepted))
                    if accepted.durability() == SendAckDurability::Persisted =>
                {
                    Some(
                        if accepted.conversation_seq > 0 && !accepted.server_msg_id.is_empty() {
                            Ok(ack.as_ref().clone())
                        } else {
                            Err(io::Error::other(
                                "persisted ACK has no server identity/sequence",
                            ))
                        },
                    )
                }
                Some(send_ack::Result::Error(error)) => Some(Err(io::Error::other(format!(
                    "server rejected send (code={})",
                    error.code
                )))),
                // Local admission / broker acceptance is not durable storage.
                _ => None,
            }
        }
        SdkEvent::Message(MessageEvent::SendFailed { client_msg_id, .. })
            if client_msg_id == id =>
        {
            Some(Err(io::Error::other("SDK reported send failure")))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_proto::common::{ErrorDetail, SendAccepted};

    fn event(result: Option<send_ack::Result>) -> SdkEvent {
        SdkEvent::Message(MessageEvent::SendAck {
            ack: Box::new(SendAck {
                client_msg_id: "wanted".into(),
                result,
                ..Default::default()
            }),
        })
    }

    fn accepted(durability: SendAckDurability, seq: u64, server_id: &str) -> SdkEvent {
        event(Some(send_ack::Result::Accepted(SendAccepted {
            durability: durability as i32,
            conversation_seq: seq,
            server_msg_id: server_id.into(),
            ..Default::default()
        })))
    }

    #[test]
    fn admission_and_broker_ack_are_not_persisted_confirmation() {
        assert!(terminal_result(&event(None), "wanted").is_none());
        assert!(
            terminal_result(
                &accepted(SendAckDurability::BrokerAccepted, 1, "server"),
                "wanted"
            )
            .is_none()
        );
        assert!(
            terminal_result(
                &accepted(SendAckDurability::Persisted, 1, "server"),
                "other"
            )
            .is_none()
        );
        assert!(
            terminal_result(
                &accepted(SendAckDurability::Persisted, 1, "server"),
                "wanted"
            )
            .unwrap()
            .is_ok()
        );
        assert!(
            terminal_result(&accepted(SendAckDurability::Persisted, 0, ""), "wanted")
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn rejection_and_send_failure_fail_closed_without_echoing_reason() {
        let error = terminal_result(
            &event(Some(send_ack::Result::Error(ErrorDetail {
                code: 7,
                message: "sensitive".into(),
                ..Default::default()
            }))),
            "wanted",
        )
        .unwrap()
        .unwrap_err();
        assert!(!error.to_string().contains("sensitive"));
        let failed = SdkEvent::Message(MessageEvent::SendFailed {
            client_msg_id: "wanted".into(),
            reason: "sensitive".into(),
        });
        assert!(terminal_result(&failed, "other").is_none());
        assert!(terminal_result(&failed, "wanted").unwrap().is_err());
    }
}
