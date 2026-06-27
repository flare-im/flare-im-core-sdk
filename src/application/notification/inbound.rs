//! Push / Sync 统一的 Notification 入站管道。

use std::sync::Arc;

use crate::application::services::MessageDeduper;
use crate::content::message_elem::Elem;
use crate::kernel::event::{EventBus, MessageEvent, NotificationEvent, SdkEvent};
use crate::model::IMMessage;

use super::registry::NotificationHandlerRegistry;
use super::types::{InboundNotificationView, should_publish_notification_as_message};

/// `NotificationContent.persistent=false`：触达但不落本地库、不参与会话摘要投影。
pub fn partition_notification_durability(
    messages: Vec<IMMessage>,
) -> (Vec<IMMessage>, Vec<IMMessage>) {
    let mut durable = Vec::new();
    let mut ephemeral = Vec::new();
    for message in messages {
        if matches!(
            message.content.as_ref(),
            Some(Elem::Notification(n)) if !n.persistent
        ) {
            ephemeral.push(message);
        } else {
            durable.push(message);
        }
    }
    (durable, ephemeral)
}

#[derive(Clone)]
pub struct NotificationInboundPipeline {
    registry: Arc<NotificationHandlerRegistry>,
    message_deduper: MessageDeduper,
    bus: EventBus,
}

impl NotificationInboundPipeline {
    pub(crate) fn new(
        registry: Arc<NotificationHandlerRegistry>,
        message_deduper: MessageDeduper,
        bus: EventBus,
    ) -> Self {
        Self {
            registry,
            message_deduper,
            bus,
        }
    }

    pub fn registry(&self) -> &Arc<NotificationHandlerRegistry> {
        &self.registry
    }

    pub async fn finish_one(&self, message: IMMessage) {
        let Some(message) = self.prepare_received_message(message).await else {
            return;
        };
        self.publish_received_message(message);
    }

    async fn prepare_received_message(&self, message: IMMessage) -> Option<IMMessage> {
        if !self.message_deduper.record_if_new(&message).await {
            return None;
        }

        if let Some(view) = InboundNotificationView::from_message(&message) {
            if !self.registry.is_empty().await {
                self.registry.dispatch(&view).await;
            }
            let should_publish_as_message = should_publish_notification_as_message(&view);
            self.bus
                .publish(SdkEvent::Notification(NotificationEvent::Received {
                    message: Box::new(message.clone()),
                }));
            return should_publish_as_message.then_some(message);
        }

        Some(message)
    }

    fn publish_received_message(&self, message: IMMessage) {
        self.bus.publish(SdkEvent::Message(MessageEvent::Received {
            message: Box::new(message),
        }));
    }

    pub async fn finish_batch(&self, messages: Vec<IMMessage>) {
        let mut received_messages = Vec::with_capacity(messages.len());
        for message in messages {
            if let Some(message) = self.prepare_received_message(message).await {
                received_messages.push(message);
            }
        }

        if received_messages.is_empty() {
            return;
        }

        self.bus
            .publish(SdkEvent::Message(MessageEvent::ReceivedBatch {
                messages: received_messages.clone(),
            }));

        for message in received_messages {
            self.publish_received_message(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::message_elem::{Elem, NotificationElem};
    use tokio::sync::mpsc;
    use tokio::time::{Duration, timeout};

    fn plain_message(server_id: &str, seq: u64) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = server_id.into();
        message.client_msg_id = format!("cli-{seq}");
        message.conversation_id = "c1".into();
        message.sender_id = "u1".into();
        message.conversation_seq = seq;
        message
    }

    fn test_pipeline(bus: EventBus) -> NotificationInboundPipeline {
        NotificationInboundPipeline::new(
            Arc::new(NotificationHandlerRegistry::new()),
            MessageDeduper::new(Some(64)),
            bus,
        )
    }

    fn notification_message(persistent: bool) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.server_id = "srv-1".into();
        message.client_msg_id = "cli-1".into();
        message.conversation_id = "c1".into();
        message.sender_id = "u1".into();
        message.conversation_seq = 1;
        message.content = Some(Elem::Notification(NotificationElem {
            title: String::new(),
            body: String::new(),
            notification_type: "social.relation.friend_request_created".into(),
            data: Default::default(),
            target_user_ids: Vec::new(),
            target_role_id: String::new(),
            notify_all: false,
            persistent,
            show_in_list: false,
            show_badge: true,
            play_sound: false,
        }));
        message
    }

    #[test]
    fn partition_splits_ephemeral_notifications() {
        let durable = notification_message(true);
        let ephemeral = notification_message(false);
        let mut plain = notification_message(true);
        plain.server_id = "plain-1".into();
        plain.content = None;

        let (durable_out, ephemeral_out) = partition_notification_durability(vec![
            durable.clone(),
            ephemeral.clone(),
            plain.clone(),
        ]);

        assert_eq!(durable_out.len(), 2);
        assert_eq!(ephemeral_out.len(), 1);
        assert_eq!(ephemeral_out[0].server_id, ephemeral.server_id);
        assert!(matches!(
            durable_out[0].content.as_ref(),
            Some(Elem::Notification(n)) if n.persistent
        ));
        assert_eq!(durable_out[1].server_id, plain.server_id);
        assert!(durable_out[1].content.is_none());
    }

    #[tokio::test]
    async fn finish_batch_publishes_batch_and_preserves_single_message_callbacks() {
        let bus = EventBus::new();
        let pipeline = test_pipeline(bus.clone());
        let (batch_tx, mut batch_rx) = mpsc::unbounded_channel();
        let (single_tx, mut single_rx) = mpsc::unbounded_channel();

        let _batch_sub = bus.on_message_batch(move |messages| {
            let _ = batch_tx.send(messages.len());
        });

        let _single_sub = bus.on_message(move |message| {
            let _ = single_tx.send(message.server_id.clone());
        });

        pipeline
            .finish_batch(vec![plain_message("srv-1", 1), plain_message("srv-2", 2)])
            .await;

        let batch_size = timeout(Duration::from_millis(200), batch_rx.recv())
            .await
            .expect("expected batch callback")
            .expect("batch callback channel closed");
        assert_eq!(batch_size, 2);

        let mut single_ids = vec![
            timeout(Duration::from_millis(200), single_rx.recv())
                .await
                .expect("expected first single callback")
                .expect("single callback channel closed"),
            timeout(Duration::from_millis(200), single_rx.recv())
                .await
                .expect("expected second single callback")
                .expect("single callback channel closed"),
        ];
        single_ids.sort();
        assert_eq!(single_ids, vec!["srv-1".to_string(), "srv-2".to_string()]);

        let extra_single = timeout(Duration::from_millis(80), single_rx.recv()).await;
        assert!(
            extra_single.is_err(),
            "single callbacks should match input size"
        );
    }
}
