//! Facade 模块
//!
//! 提供所有 Facade 相关的 API

pub mod facade;
pub mod message_facade;
pub mod conversation_facade;
pub mod event_subscription_facade;
pub mod default_message_handler;

pub use facade::ImCoreSdk;
pub use message_facade::MessageFacade;
pub use conversation_facade::ConversationFacade;
pub use event_subscription_facade::EventSubscriptionFacade;
pub use default_message_handler::DefaultMessageHandler;

// 重新导出 MentionInfo 和 MentionInfoType（从领域服务）
pub use crate::domain::service::{MentionInfo, MentionInfoType};
