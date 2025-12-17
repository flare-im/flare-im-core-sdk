//! 命令和查询处理器
//!
//! 负责处理命令和查询，协调领域服务和基础设施

pub mod connection_command_handler;
pub mod message_command_handler;
pub mod message_query_handler;
pub mod session_command_handler;
pub mod session_query_handler;
pub mod sync_command_handler;
pub mod sync_query_handler;

// 重新导出
pub use connection_command_handler::ConnectionCommandHandler;
pub use message_command_handler::MessageCommandHandler;
pub use message_query_handler::MessageQueryHandler;
pub use session_command_handler::SessionCommandHandler;
pub use session_query_handler::SessionQueryHandler;
pub use sync_command_handler::SyncCommandHandler;
pub use sync_query_handler::SyncQueryHandler;
