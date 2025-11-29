//! 生命周期管理
//!
//! 提供统一的组件生命周期管理，支持优雅关闭和资源清理

mod lifecycle;
mod manager;

pub use lifecycle::*;
pub use manager::*;

