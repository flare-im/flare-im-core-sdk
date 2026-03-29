//! 领域事件 — 与 proto Event / EventType 一致

pub use flare_proto::common::{Event, EventType};
pub use flare_proto::common::{
    MarkEvent, MessageDeleteEvent, PinEvent, ReactionEvent, ReadReceiptEvent, TypingEvent,
    UnmarkEvent, UnpinEvent,
};
