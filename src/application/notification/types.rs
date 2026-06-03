//! IM 下行 Notification 视图与 Handler 契约。

use std::collections::HashMap;

use async_trait::async_trait;

use crate::model::IMMessage;
use crate::model::message_elem::Elem;

/// 从 [`IMMessage`] 投影的 Notification 视图（无业务语义）。
#[derive(Debug, Clone)]
pub struct InboundNotificationView<'a> {
    pub message: &'a IMMessage,
    pub notification_type: &'a str,
    pub data: &'a HashMap<String, String>,
    pub persistent: bool,
    pub show_in_list: bool,
    pub show_badge: bool,
    pub play_sound: bool,
}

impl<'a> InboundNotificationView<'a> {
    pub fn from_message(message: &'a IMMessage) -> Option<Self> {
        let Some(Elem::Notification(n)) = message.content.as_ref() else {
            return None;
        };
        Some(Self {
            message,
            notification_type: n.notification_type.trim(),
            data: &n.data,
            persistent: n.persistent,
            show_in_list: n.show_in_list,
            show_badge: n.show_badge,
            play_sound: n.play_sound,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationHandleResult {
    Handled,
    Ignored,
}

/// 业务 SDK 实现的 IM 下行 Notification 处理器。
#[async_trait]
pub trait NotificationHandler: Send + Sync {
    fn matches(&self, notification_type: &str) -> bool;

    async fn handle(&self, notification: &InboundNotificationView<'_>) -> NotificationHandleResult;
}

pub(crate) fn should_publish_notification_as_message(view: &InboundNotificationView<'_>) -> bool {
    view.persistent || view.show_in_list
}
