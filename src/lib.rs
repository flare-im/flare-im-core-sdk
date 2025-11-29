//! Flare IM Client SDK
//!
//! 跨平台的即时通讯客户端SDK，支持Web、PC桌面、Android、iOS、鸿蒙等平台。
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

pub mod client;
pub mod config;
pub mod connection;
pub mod protocol;
pub mod service;
pub mod storage;
pub mod model;
pub mod event;
pub mod handler;
pub mod observer;
pub mod task;
pub mod error;
pub mod lifecycle;
pub mod platform;
#[cfg(feature = "extensions")]
pub mod extension;
pub use client::{FlareIMClient, LoginResult};
pub use config::{ClientConfig, ClientConfigBuilder, DevicePlatform};
pub use model::{
    Message, ExtendedMessage,
    SessionSummary, ExtendedSessionSummary,
    SyncCursor, SyncResult,
    MessageBuilder,
    MessageExtension, MessageLocalState,
    SessionExtension,
    UserExtension,
    ExtensionProvider, ExtensionCache,
};

#[cfg(feature = "extensions")]
pub use extension::{
    ExtensionInfoManager,
    StorageExtensionProvider,
    MemoryExtensionProvider,
    StorageExtensionCache,
    MemoryExtensionCache,
};
pub use event::{Event, EventBus, ConnectionEvent, MessageEvent, SessionEvent, SyncEvent};
pub use storage::{StorageBackend, SessionFilter, SessionUpdate, MessageState};
pub use service::message::SendOptions;
pub use service::message::MessagePriority;
pub use service::crypto::{CryptoService, NoopCrypto, AesCrypto};
pub use observer::{MessageObserver, ArcMessageObserver};
pub use error::{SDKError, SDKResult};
