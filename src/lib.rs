//! Flare IM Core SDK
//!
//! 全端统一 IM Core SDK，基于 DDD + CQRS + FSM + Unified Sync + Storage Layer 架构
//!
//! ## 架构层次
//!
//! ```
//! Application / UI
//!        ↓
//! SDK Application Layer
//! (Facade / CommandBus / QueryBus / FSM / Sync Coordinator)
//!        ↓
//! Domain Layer (DDD)
//! (Aggregate / Entity / Domain Event)
//!        ↓
//! Infrastructure Layer
//! (LocalStore / Network / Clock)
//! ```
//!
//! ## Core Bounded Context
//!
//! - **Session**: 登录态 / Token
//! - **Connection**: 长连接 / 心跳
//! - **Sync**: 统一同步引擎
//! - **Message**: 消息生命周期
//! - **Conversation**: 会话 / 未读数
//!
//! ## 设计原则
//!
//! 1. 状态是第一公民
//! 2. 所有写操作必须可回放
//! 3. 读写物理隔离（CQRS）
//! 4. FSM 是唯一状态迁移入口
//! 5. 存储层不包含业务逻辑

// 共享模块
pub mod shared;

// C ABI 包装层（用于自动生成各平台绑定）
#[cfg(feature = "ffi")]
pub mod ffi;

// ============================================================================
// Core SDK 架构模块
// ============================================================================

// 配置模块
pub mod config;

// Domain Layer - 领域层
pub mod domain {
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
}

// Application Layer - 应用层
// 使用 application/mod.rs 中定义的结构
pub mod application;

// Infrastructure Layer - 基础设施层
pub mod infrastructure;

// Interface Layer - 接口层
pub mod interface;

// Extension 模块（可选）
#[cfg(feature = "extensions")]
pub mod extensions;

// 预导出常用类型
pub mod prelude {
    pub use crate::application::fsm::*;
    pub use crate::application::sync_coordinator::*;
    pub use crate::domain::event::*;
    pub use crate::domain::repository::*;
    pub use crate::infrastructure::storage::*;
    pub use crate::interface::facade::*;
    pub use crate::interface::event::*;
    pub use crate::shared::error::*;
    
    #[cfg(feature = "extensions")]
    pub use crate::extensions::*;
}
