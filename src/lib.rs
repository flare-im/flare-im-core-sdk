//! Flare IM Client SDK
//!
//! 跨平台的即时通讯客户端SDK，支持Web、PC桌面、Android、iOS、鸿蒙等平台。
//!
//! ## 安全性说明
//!
//! FFI 模块（`ffi`）包含 C ABI 代码，虽然使用了 `#[no_mangle]` 和原始指针，
//! 但所有公共 API 都是安全的。所有 unsafe 操作都封装在安全包装层中。

#![allow(unsafe_code)] // FFI 模块需要 unsafe，但已封装在安全包装层中
//!
//! ## 架构设计
//!
//! SDK 采用分层架构设计，参考顶级 IM SDK（Telegram、微信、WhatsApp）：
//!
//! - **API 层** (`api/`): 对外统一 API
//! - **应用层** (`application/`): 业务编排
//! - **领域层** (`domain/`): 核心业务逻辑
//! - **基础设施层** (`infrastructure/`): 技术实现
//! - **共享层** (`shared/`): 跨层共享功能
//!
//! ## 快速开始
//!
//! ```rust,no_run
//! use flare_im_core_sdk::{FlareIMClient, ClientConfig, ClientConfigBuilder};
//! use flare_core::common::config_types::TransportProtocol;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // 创建客户端配置
//!     let config = ClientConfig::builder()
//!         .server_url("wss://im.example.com")
//!         .user_id("user_123")
//!         .device_id("device_456")
//!         .token("your_token")
//!         .protocols(vec![
//!             TransportProtocol::QUIC,
//!             TransportProtocol::WebSocket,
//!         ])
//!         .build()?;
//!
//!     // 创建客户端
//!     let client = FlareIMClient::new(config).await?;
//!
//!     // 登录
//!     let login_result = client.login("user_123", "your_token").await?;
//!     println!("登录成功: {:?}", login_result);
//!
//!     // 发送消息
//!     let message_id = client.send_message(
//!         "session_123",
//!         flare_proto::MessageContent {
//!             content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
//!                 flare_proto::TextContent {
//!                     text: "Hello, World!".to_string(),
//!                     mentions: vec![],
//!                 }
//!             )),
//!         },
//!     ).await?;
//!     println!("消息已发送: {}", message_id);
//!
//!     // 获取会话列表
//!     let sessions = client.get_sessions(flare_im_core_sdk::SessionFilter::default()).await?;
//!     println!("会话数量: {}", sessions.len());
//!
//!     Ok(())
//! }
//! ```

// 分层架构模块
pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod shared;

// C ABI 包装层（用于自动生成各平台绑定）
#[cfg(feature = "ffi")]
pub mod ffi;

// 重新导出公共 API
pub use api::{FlareIMClient, LoginResult};
#[cfg(feature = "extensions")]
pub use domain::extension::{
    ExtensionCache, ExtensionProvider, MessageExtension, MessageLocalState, SessionExtension,
    UserExtension,
};
// ExtendedMessage 已删除，使用 DomainMessage + Extension 替代
// #[cfg(feature = "extensions")]
// pub use domain::message::ExtendedMessage;
pub use domain::message::Message;
// 使用 domain 层的 MessageBuilder（支持构建完整 Message）
pub use domain::MessageBuilder;
// SessionBuilder 暂时保留在 application 层（逐步迁移到 domain 层）
// pub use application::session::SessionBuilder; // 暂时注释，等待迁移完成
#[cfg(feature = "extensions")]
pub use domain::session::ExtendedSessionSummary;
pub use domain::session::SessionSummary;
pub use domain::sync::{SyncCursor, SyncResult};
pub use shared::config::{ClientConfig, ClientConfigBuilder, DevicePlatform};

#[cfg(feature = "extensions")]
pub use shared::extension::{
    ExtensionInfoManager, MemoryExtensionCache, MemoryExtensionProvider, StorageExtensionCache,
    StorageExtensionProvider,
};

pub use application::{AesCrypto, CryptoService, NoopCrypto};
pub use infrastructure::event::{
    ConnectionEvent, Event, EventBus, MessageEvent, SessionEvent, SyncEvent,
};
pub use infrastructure::storage::{MessageState, SessionFilter, SessionUpdate, StorageBackend};
pub use shared::error::{SDKError, SDKResult};
pub use shared::observer::{ArcMessageObserver, MessageObserver};
