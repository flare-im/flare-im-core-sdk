//! 存储模块
//!
//! 实现 EventStore、ReadStore、SnapshotStore

pub mod event_store;
pub mod read_store;
pub mod snapshot_store;
pub mod event_projection;
pub mod media_cache;

// 导出常用类型（所有平台都导出 Memory 实现用于测试）
pub use event_store::MemoryEventStore;
pub use read_store::MemoryReadStore;
pub use snapshot_store::MemorySnapshotStore;
pub use event_projection::{EventProjector, EventProjectorBuilder};

// 平台特定实现
#[cfg(not(target_arch = "wasm32"))]
pub use event_store::SqliteEventStore;
#[cfg(not(target_arch = "wasm32"))]
pub use read_store::SqliteReadStore;
#[cfg(not(target_arch = "wasm32"))]
pub use snapshot_store::SqliteSnapshotStore;

#[cfg(target_arch = "wasm32")]
pub use event_store::IndexedDbEventStore;
#[cfg(target_arch = "wasm32")]
pub use read_store::IndexedDbReadStore;
#[cfg(target_arch = "wasm32")]
pub use snapshot_store::IndexedDbSnapshotStore;
