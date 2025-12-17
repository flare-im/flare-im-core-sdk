//! 会话领域模块
//!
//! 遵循 DDD 原则，包含：
//! - model: 领域模型（聚合根、实体、值对象）
//! - service: 领域服务接口和实现
//! - repository: 仓储接口
//! - event: 领域事件

pub mod event;
pub mod model;
pub mod repository;
pub mod service;

// 重新导出常用类型
pub use event::*;
pub use model::*;
pub use repository::*;
pub use service::*;

// 重新导出 SessionSummary 和 SessionSummaryProto
pub use flare_proto::common::SessionSummary as SessionSummaryProto;
pub use model::SessionSummary;
