pub mod fsm;

// 命令定义（Command DTOs）
pub mod commands;

// 查询定义（Query DTOs）
pub mod queries;

// CQRS Handler（编排层）
pub mod handlers;

pub mod ports;

// 同步协调器
pub mod sync_coordinator;

// Extension 机制
pub mod extension;

// 注意：旧的 command 和 query 模块已删除，请使用新的 handlers

// 导出主要的处理器
pub use handlers::{CommandHandler, QueryHandler};
