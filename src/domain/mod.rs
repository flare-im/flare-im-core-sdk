//! Domain Layer - 领域层
//!
//! 职责：包含所有业务逻辑和领域模型

pub mod session;
pub mod connection;
pub mod message;
pub mod conversation;
pub mod sync;

// 领域事件
pub mod event;

// 领域服务
pub mod service;

// 仓储接口（Port）
pub mod repository;

// 常量定义
pub mod constants;

// 消息队列
pub mod message_queue;
