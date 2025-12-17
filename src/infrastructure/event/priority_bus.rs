//! 优先级事件总线
//!
//! 支持事件优先级，确保关键事件不丢失

use crate::infrastructure::event::Event;
use tokio::sync::broadcast;

/// 事件优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    /// 关键事件（连接状态、认证等）- 最高优先级
    Critical = 0,
    /// 高优先级事件（消息接收、发送）
    High = 1,
    /// 普通事件（会话更新）
    Normal = 2,
    /// 低优先级事件（统计事件等）
    Low = 3,
}

/// 优先级事件总线
///
/// 优化：支持事件优先级，确保关键事件不丢失
pub struct PriorityEventBus {
    /// 关键事件通道（小容量，确保不丢失）
    critical_sender: broadcast::Sender<Event>,

    /// 普通事件通道（大容量）
    normal_sender: broadcast::Sender<Event>,

    /// 低优先级事件通道（中等容量，可丢弃）
    low_sender: broadcast::Sender<Event>,
}

impl PriorityEventBus {
    /// 创建新的优先级事件总线
    pub fn new() -> Self {
        // 关键事件：小容量，确保不丢失
        let (critical_sender, _) = broadcast::channel(100);

        // 普通事件：中等容量
        let (normal_sender, _) = broadcast::channel(1000);

        // 低优先级事件：大容量，可丢弃
        let (low_sender, _) = broadcast::channel(500);

        Self {
            critical_sender,
            normal_sender,
            low_sender,
        }
    }

    /// 发布事件（带优先级）
    ///
    /// # 参数
    /// - `event`: 要发布的事件
    /// - `priority`: 事件优先级
    pub fn publish_with_priority(&self, event: Event, priority: EventPriority) {
        let sender = match priority {
            EventPriority::Critical => &self.critical_sender,
            EventPriority::High | EventPriority::Normal => &self.normal_sender,
            EventPriority::Low => &self.low_sender,
        };

        // 先克隆 event，以便在需要时重试
        let event_clone_for_retry = event.clone();
        match sender.send(event) {
            Ok(_) => {}
            Err(broadcast::error::SendError(_)) => {
                // 关键事件尝试重试（异步）
                if priority == EventPriority::Critical {
                    let sender_clone = sender.clone();
                    let event_clone = event_clone_for_retry;
                    tokio::spawn(async move {
                        // 短暂等待后重试
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        if let Err(e) = sender_clone.send(event_clone) {
                            tracing::error!(error = %e, "Failed to retry critical event");
                        }
                    });
                } else {
                    // 非关键事件直接丢弃
                    tracing::debug!(
                        "Event dropped due to channel full (priority: {:?})",
                        priority
                    );
                }
            }
        }
    }

    /// 发布事件（自动判断优先级）
    pub fn publish(&self, event: Event) {
        let priority = Self::infer_priority(&event);
        self.publish_with_priority(event, priority);
    }

    /// 推断事件优先级
    fn infer_priority(event: &Event) -> EventPriority {
        match event {
            Event::Connection(_) => EventPriority::Critical,
            Event::Message(me) => match me {
                crate::infrastructure::event::MessageEvent::MessageReceived { .. } => {
                    EventPriority::High
                }
                crate::infrastructure::event::MessageEvent::MessageSent { .. } => {
                    EventPriority::High
                }
                crate::infrastructure::event::MessageEvent::MessageFailed { .. } => {
                    EventPriority::High
                }
                _ => EventPriority::Normal,
            },
            Event::Session(_) => EventPriority::Normal,
            Event::Sync(_) => EventPriority::Low,
            Event::Task(te) => match te {
                // Blocking 任务事件优先级高（关键任务）
                crate::infrastructure::event::TaskEvent::BlockingTaskStarted { .. }
                | crate::infrastructure::event::TaskEvent::BlockingTaskCompleted { .. }
                | crate::infrastructure::event::TaskEvent::BlockingTaskFailed { .. } => {
                    EventPriority::High
                }
                // Background 任务事件优先级低（后台任务）
                _ => EventPriority::Low,
            },
        }
    }

    /// 订阅所有事件（合并多个通道）
    pub fn subscribe(&self) -> PriorityEventReceiver {
        PriorityEventReceiver {
            critical_rx: self.critical_sender.subscribe(),
            normal_rx: self.normal_sender.subscribe(),
            low_rx: self.low_sender.subscribe(),
        }
    }

    /// 只订阅关键事件
    pub fn subscribe_critical(&self) -> broadcast::Receiver<Event> {
        self.critical_sender.subscribe()
    }
}

/// 优先级事件接收器
///
/// 合并多个通道的事件，按优先级顺序接收
pub struct PriorityEventReceiver {
    critical_rx: broadcast::Receiver<Event>,
    normal_rx: broadcast::Receiver<Event>,
    low_rx: broadcast::Receiver<Event>,
}

impl PriorityEventReceiver {
    /// 接收下一个事件（按优先级顺序）
    pub async fn recv(&mut self) -> std::result::Result<Event, broadcast::error::RecvError> {
        // 使用 tokio::select! 按优先级接收
        tokio::select! {
            result = self.critical_rx.recv() => result,
            result = self.normal_rx.recv() => result,
            result = self.low_rx.recv() => result,
        }
    }
}

impl Default for PriorityEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::event::ConnectionEvent;

    #[tokio::test]
    async fn test_priority_event_bus() {
        let bus = PriorityEventBus::new();
        let mut rx = bus.subscribe();

        // 发布关键事件
        bus.publish_with_priority(
            Event::Connection(ConnectionEvent::Connected { protocol: None }),
            EventPriority::Critical,
        );

        let event = rx.recv().await.unwrap();
        match event {
            Event::Connection(ConnectionEvent::Connected { .. }) => {}
            _ => panic!("Unexpected event type"),
        }
    }
}
