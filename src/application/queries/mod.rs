//! 查询定义（CQRS 读侧）
//!
//! 定义所有读操作的数据结构，遵循 CQRS 原则

pub mod message;
pub mod session;
pub mod sync;

// 重新导出
pub use message::*;
pub use session::*;
pub use sync::*;
