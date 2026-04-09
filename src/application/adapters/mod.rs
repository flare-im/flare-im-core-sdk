//! 应用层适配器/服务。
//!
//! 仅保留仍需要作为协议适配器或独立基础服务存在的实现。

mod media_service;

mod sync_protocol_adapter;

pub use media_service::MediaService;
pub use sync_protocol_adapter::SyncProtocolAdapter;
