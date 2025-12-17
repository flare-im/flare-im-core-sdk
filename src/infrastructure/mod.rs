//! 基础设施层
//!
//! 技术实现，具体技术选型
//!
//! 原则：
//! - 实现领域层定义的接口
//! - 可替换的技术实现
//! - 处理具体的技术细节

pub mod connection;
pub mod event;
pub mod handler;
pub mod persistence;
pub mod protocol;
pub mod storage;
pub mod task;

// 明确导出，避免 ambiguous glob re-exports 警告
pub use connection::{
    ConnectionManager, ConnectionState, ConnectionStateMachine, MemoryStatePersistence,
    NetworkQuality, ReconnectStrategy, SDKMessageListener, StateHistory, StatePersistence,
    StateSnapshot, StateTransition,
};
pub use event::{
    ConnectionEvent, Event, EventBus, MessageEvent, SessionEvent, SyncEvent, TaskEvent,
};
pub use handler::MessageFrameHandler;
pub use protocol::{FrameBuilder, RequestManager};
pub use storage::{
    CacheStats, CachedStorage, CachedStorageBackend, LastMessageUpdate, MediaInfo, MediaType,
    MediaUploadOptions, MediaUploadResult, MediaUploadService, MessageState, PendingMessage,
    PendingMessageQueue, PendingMessageQueueConfig, QueryCache, SessionFilter, SessionUpdate,
    StorageBackend, UploadProgress,
};
pub use task::{
    ResourceCleaner, ResourceManager, ResourceStats, ResourceType, SyncContext, SyncTask,
    SyncTaskExecutor, TaskInfo, TaskManager, TaskManagerBuilder, TaskResult, TaskScheduler,
    TaskSchedulerConfig, TaskSchedulerStats, TaskStatus, TaskType, TaskType as ManagerTaskType,
};
