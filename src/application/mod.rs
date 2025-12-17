//! 应用服务层（Application Layer）
//!
//! ## 架构设计（DDD + CQRS）
//!
//! 应用层负责业务编排，协调领域服务和基础设施，不包含业务逻辑。
//!
//! ### 目录结构
//!
//! ```
//! application/
//! ├── commands/          # 命令定义（CQRS 写侧）
//! ├── queries/           # 查询定义（CQRS 读侧）
//! ├── handlers/          # 命令和查询处理器
//! ├── services/          # 应用服务（业务编排）
//! ├── receivers/         # 服务端消息/命令接收处理
//! └── crypto.rs          # 加密服务
//! ```
//!
//! ### 设计原则
//!
//! 1. **CQRS 严格分离**：命令（写）和查询（读）完全分离
//! 2. **薄应用层**：只负责编排，不包含业务逻辑
//! 3. **无状态设计**：所有服务都是无状态的，可并发使用
//! 4. **事件驱动**：通过事件总线解耦各模块
//!
//! ### 数据流向
//!
//! **写操作（Command）**：
//! API 层 → Command → CommandHandler → DomainService → Repository → Storage
//!
//! **读操作（Query）**：
//! API 层 → Query → QueryHandler → Repository → Storage → API 层
//!
//! **服务端推送**：
//! Infrastructure → Receiver → DomainService → Repository → EventBus → API 层

pub mod commands;
pub mod crypto;
pub mod handlers;
// message 模块已删除，media_upload 已移到 infrastructure/storage
pub mod queries;
pub mod receivers;
pub mod services;
pub mod session;
pub mod sync;
pub mod vo;

// 重新导出主要类型
pub use crypto::{AesCrypto, CryptoService, NoopCrypto};

// 重新导出命令和查询
pub use commands::*;
pub use queries::*;

// 重新导出处理器
pub use handlers::*;

// 重新导出应用服务
pub use services::*;

// 重新导出接收器
pub use receivers::*;

// 重新导出视图模型
pub use vo::*;
