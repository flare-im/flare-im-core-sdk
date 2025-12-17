//! 任务系统
//!
//! 统一的任务管理和执行系统
//!
//! 合并了原来的 `core/task_manager` 和 `task/` 模块

pub mod builtin;
pub mod config;
pub mod executor;
pub mod manager;
pub mod message_retry;
pub mod priority_task;
pub mod resource_manager;
pub mod scheduler;
pub mod scheduler_stats;
pub mod standard;
pub mod task;

pub use builtin::*;
pub use executor::*;
pub use manager::{TaskInfo, TaskManager, TaskManagerBuilder, TaskType};
pub use message_retry::MessageRetryTask;
pub use priority_task::*;
pub use resource_manager::{ResourceCleaner, ResourceManager, ResourceStats, ResourceType};
pub use scheduler::{TaskScheduler, TaskSchedulerConfig, TaskSchedulerStats};
pub use scheduler_stats::{TaskSchedulerInternalStats, TaskSchedulerPerformanceSnapshot};
pub use standard::*;
pub use task::*;
