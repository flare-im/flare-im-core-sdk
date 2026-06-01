//! Push / Sync 统一的 Notification 入站管道。

use std::sync::Arc;

use crate::application::message_deduper::MessageDeduper;
use crate::event::{EventBus, MessageEvent, NotificationEvent, SdkEvent};
use crate::model::IMMessage;
use crate::model::message_elem::Elem;

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
        if !self.message_deduper.record_if_new(&message).await {
            return;
        }

        if let Some(view) = InboundNotificationView::from_message(&message) {
            if !self.registry.is_empty().await {
                self.registry.dispatch(&view).await;
            }
            self.bus
                .publish(SdkEvent::Notification(NotificationEvent::Received {
                    message: Box::new(message.clone()),
                }));
            if should_publish_notification_as_message(&view) {
                self.bus.publish(SdkEvent::Message(MessageEvent::Received {
                    message: Box::new(message),
                }));
            }
            return;
        }

        self.bus.publish(SdkEvent::Message(MessageEvent::Received {
            message: Box::new(message),
        }));
    }

    pub async fn finish_batch(&self, messages: Vec<IMMessage>) {
        for message in messages {
            self.finish_one(message).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::message_elem::{Elem, NotificationElem};

    fn notification_message(persistent: bool) -> IMMessage {
        IMMessage {
            server_id: "srv-1".into(),
            client_msg_id: "cli-1".into(),
            conversation_id: "c1".into(),
            conversation_type: 0,
            channel_id: String::new(),
            sender_id: "u1".into(),
            source: 0,
            seq: 1,
            timestamp: 0,
            client_timestamp: 0,
            message_type: 0,
            content: Some(Elem::Notification(NotificationElem {
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
            })),
            content_bytes: Vec::new(),
            sender_name: String::new(),
            sender_avatar: String::new(),
            sender_display_name: String::new(),
            reply_to: None,
            quote_preview: None,
            status: 0,
            is_read: false,
            is_recalled: false,
            is_edited: false,
            burn_enabled: false,
            burn_after_read_seconds: None,
            burn_status: 0,
            first_read_at: None,
            burn_at: None,
            burned_at: None,
            mention_users: Vec::new(),
            mention_all: false,
            offline_push_info: None,
            extra: Default::default(),
            extensions: Default::default(),
            reactions: Vec::new(),
            version: 0,
            updated_at: 0,
            local_state: Default::default(),
        }
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
}
