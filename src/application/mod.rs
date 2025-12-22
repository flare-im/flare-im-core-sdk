//! Application Layer - 应用层
//!
//! 职责：编排领域服务，处理应用层逻辑
//! 不包含业务逻辑，只负责编排

// FSM 状态机
pub mod fsm;

// 命令定义（Command DTOs）
pub mod commands;

// 查询定义（Query DTOs）
pub mod queries;

// CQRS Handler（编排层）
pub mod handlers;

// 同步协调器
pub mod sync_coordinator;

// Extension 机制
pub mod extension;

// 注意：旧的 command 和 query 模块已删除，请使用新的 handlers

// 导出主要的处理器
pub use handlers::{CommandHandler, QueryHandler};
