//! 视图模型（View Objects / Value Objects）
//!
//! 用于 API 层和应用层之间的数据传输
//!
//! ## 设计原则
//!
//! 1. **视图模型与领域模型分离**：VO 是专门为 API 层设计的，不直接暴露领域模型
//! 2. **简化数据结构**：VO 只包含 API 层需要的数据，不包含领域逻辑
//! 3. **序列化友好**：VO 应该易于序列化为 JSON 等格式
//! 4. **转换方法**：提供从领域模型到 VO 的转换方法

pub mod message;
pub mod session;
pub mod sync;

pub use message::*;
pub use session::*;
pub use sync::*;
