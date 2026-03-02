//! 事件转发模块
//!
//! 将 SDK 事件自动转发到 Tauri 前端
//!
//! 按事件类型拆分：
//! - `message`: 消息事件（创建、发送、已读、撤回等）
//! - `connection`: 连接事件（连接、断开、重连等）
//! - `session`: 会话事件（登录、登出、过期等）
//! - `conversation`: 会话事件（创建、更新、已读等）
//! - `sync`: 同步事件（Bootstrap、增量同步等）

pub mod message;
pub mod connection;
pub mod session;
pub mod conversation;
pub mod sync;
