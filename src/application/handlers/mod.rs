//! CQRS Handler（编排层）
//!
//! 职责：编排领域服务，处理应用层逻辑
//! 不包含业务逻辑，只负责编排

mod command_handler;
mod query_handler;
mod message_command_handler;
mod conversation_command_handler;
mod session_command_handler;
mod message_query_handler;
mod conversation_query_handler;
mod session_query_handler;
mod sync_handler;
mod conversation_sync_handler;
mod custom_data_handler;
mod network_message_dispatcher;

pub use command_handler::CommandHandler;
pub use query_handler::QueryHandler;
pub use message_command_handler::MessageCommandHandler;
pub use conversation_command_handler::ConversationCommandHandler;
pub use session_command_handler::SessionCommandHandler;
pub use message_query_handler::MessageQueryHandler;
pub use conversation_query_handler::ConversationQueryHandler;
pub use session_query_handler::SessionQueryHandler;
pub use sync_handler::SyncHandler;
pub use conversation_sync_handler::ConversationSyncHandler;
pub use custom_data_handler::CustomDataHandler;
pub use network_message_dispatcher::NetworkMessageDispatcher;
