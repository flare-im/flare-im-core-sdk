//! 事件通知 API 实现

use crate::api::FlareIMClient;
use crate::api::traits::EventApi;
use crate::infrastructure::event::EventBus;
use crate::shared::observer::ArcMessageObserver;
use std::sync::Arc;

impl EventApi for FlareIMClient {
    fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }

    async fn register_message_observer(&self, observer: ArcMessageObserver) {
        self.observer_registry.register(observer).await;
    }
}
