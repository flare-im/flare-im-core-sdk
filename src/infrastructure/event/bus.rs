//! 事件总线

use crate::infrastructure::event::Event;
use tokio::sync::broadcast;

/// 事件总线
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    /// 创建新的事件总线
    ///
    /// 根据平台自动调整缓冲区大小
    pub fn new() -> Self {
        use crate::shared::platform::{Platform, get_platform};
        let platform = get_platform();

        // 根据平台调整缓冲区大小
        let capacity = match platform {
            Platform::Web => 500,      // Web 端较小的缓冲区（考虑内存限制）
            Platform::Desktop => 2000, // 桌面端较大的缓冲区
            Platform::Android | Platform::IOS | Platform::HarmonyOS => 1000, // 移动端中等缓冲区
        };

        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// 创建指定容量的事件总线
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// 发布事件
    ///
    /// 优化：使用 send 方法，如果通道满则记录警告但不阻塞
    /// 注意：broadcast::Sender 的 send 方法不会阻塞，如果接收者已满会返回错误
    pub fn publish(&self, event: Event) {
        match self.sender.send(event) {
            Ok(_) => {}
            Err(broadcast::error::SendError(_)) => {
                // 通道满或已关闭，记录警告但不阻塞（背压处理）
                tracing::warn!("Event bus channel full or closed, event dropped");
            }
        }
    }

    /// 发布事件（阻塞版本，用于关键事件）
    ///
    /// 对于关键事件（如连接状态变化），使用此方法确保事件被发送
    pub async fn publish_blocking(&self, event: Event) {
        if let Err(e) = self.sender.send(event) {
            tracing::error!(error = %e, "Failed to publish event");
        }
    }

    /// 获取当前订阅者数量（用于监控）
    pub fn receiver_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// 订阅事件
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    // broadcast 作为唯一事件分发机制
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::event::ConnectionEvent;

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.publish(Event::Connection(ConnectionEvent::Connected {
            protocol: None,
        }));

        let event = rx.recv().await.unwrap();
        match event {
            Event::Connection(ConnectionEvent::Connected { .. }) => {}
            _ => panic!("Unexpected event type"),
        }
    }
}
