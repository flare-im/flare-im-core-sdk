//! Notification Handler 注册表：按 `matches` 顺序 dispatch。

use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::warn;

use super::types::{InboundNotificationView, NotificationHandler};

pub struct NotificationHandlerRegistry {
    handlers: RwLock<Vec<Arc<dyn NotificationHandler>>>,
}

impl NotificationHandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(Vec::new()),
        }
    }

    pub async fn register(&self, handler: Arc<dyn NotificationHandler>) {
        self.handlers.write().await.push(handler);
    }

    pub async fn is_empty(&self) -> bool {
        self.handlers.read().await.is_empty()
    }

    pub async fn dispatch(&self, notification: &InboundNotificationView<'_>) {
        let handlers = self.handlers.read().await;
        for handler in handlers.iter() {
            if handler.matches(notification.notification_type) {
                let _ = handler.handle(notification).await;
                return;
            }
        }
        warn!(
            notification_type = %notification.notification_type,
            server_id = %notification.message.server_id,
            "notification: no handler registered"
        );
    }
}

impl Default for NotificationHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
