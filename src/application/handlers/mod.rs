//! 应用层业务处理器
//!
//! 消息与会话的上层业务编排，委托 Command/Query 与 Store，通过 EventBus 与 core 协同。

mod conversation_flow;
mod conversation_query_handler;
mod message_builder_handler;
mod message_engine;
mod message_query_handler;
mod sync_handler;

pub use conversation_flow::ConversationFlow;
pub use conversation_query_handler::ConversationQueryHandler;
pub use message_builder_handler::MessageBuilderHandler;
pub use message_engine::MessageEngine;
pub use message_query_handler::MessageQueryHandler;
pub use sync_handler::SyncHandler;
