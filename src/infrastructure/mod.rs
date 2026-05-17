//! 基础设施层 — 与 orchestrator infrastructure 对齐
//!
//! 持久化（persistence）、协议（protocol）、传输（transport）统一归属本层。

pub mod persistence;
pub mod protocol;
pub mod transport;

// 常用类型统一导出，便于上层通过 infrastructure::* 使用
pub use crate::domain::{ConversationStore, MessageStore, SyncCursorStore};
pub use persistence::{
    LayeredPendingSendStore, MemoryPendingSendStore, MemoryUserProfileStore, StoreProvider,
};
pub use protocol::{Codec, PacketSender, ProtobufCodec};
pub use transport::{HttpClient, SocketHandler, SocketTransport};
