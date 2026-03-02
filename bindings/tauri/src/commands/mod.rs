//! Tauri 命令实现
//!
//! 按功能模块拆分：
//! - `lifecycle`: 生命周期管理（初始化、连接、登录、登出）
//! - `message`: 消息操作（发送、编辑、删除、撤回、反应等）
//! - `conversation`: 会话操作（获取列表、标记已读、输入状态等）
//! - `sync`: 同步操作（Bootstrap Sync、增量同步等）

pub mod lifecycle;
pub mod message;
pub mod conversation;
pub mod sync;
