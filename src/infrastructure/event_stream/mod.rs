//! 事件流处理（以 proto Event 为唯一线缆数据源）
//!
//! 统一消费 EventEnvelope（推送/同步），按 EventType 分发：
//! - EVENT_MESSAGE → 入队 MessageQueue，发布 Message.Created
//! - 撤回/编辑/删除/已读/反应/置顶/标记等 → 转 DomainEvent 发布到 EventBus

mod processor;

pub use processor::EventStreamProcessor;
