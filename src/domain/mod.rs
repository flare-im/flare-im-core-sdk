//! 领域层：消息、会话、用户展示模型
//!
//! 所有模型均包含展示用**显示名称（display_name）**与**头像（avatar_url）**，
//! 由本地 UserProfile 缓存或同步填充，供 UI 列表与详情展示。

pub mod conversation;
mod message;
mod models;
mod repository;
mod sync;

pub use conversation::*;
pub use message::*;
pub use models::*;
pub use repository::*;
pub use sync::*;
