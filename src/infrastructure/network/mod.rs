//! 网络模块
//!
//! 基于 flare-core 实现的网络连接层，负责与服务器的通信
//!
//! # 模块结构
//!
//! - `client` - 网络客户端实现（连接、发送、状态查询）
//! - `listener` - 消息监听器实现（处理网络事件回调，支持 ServerPacket 解析）
//! - `parser` - 消息解析器（Frame -> Message，支持 ServerPacket/MessageEnvelope/Message）
//! - `packet` - ServerPacket 解析器（解析统一传输包的不同 payload 类型）
//! - `types` - 类型定义（NetworkMessage, ConnectionEvent）
//!
//! # 架构设计
//!
//! 对标微信、Telegram、飞书的网络层设计：
//!
//! 1. **连接管理**：支持 WebSocket/QUIC 双协议，自动协议竞速
//! 2. **消息收发**：基于 Frame 的二进制协议，支持可靠传输
//! 3. **协议支持**：支持 `ServerPacket`（统一传输包）和 `MessageEnvelope`（向后兼容）
//! 4. **ACK 处理**：由 `send_frame_and_wait` 自动处理，无需手动匹配
//! 5. **事件驱动**：通过 Channel 传递网络事件，解耦网络层和应用层
//! 6. **自定义数据**：支持 `CustomPushData` 自定义推送数据
//!
//! # 使用示例
//!
//! ```rust
//! use flare_im_core_sdk::infrastructure::network::NetworkClient;
//!
//! // 创建客户端
//! let (client, mut message_rx, mut connection_rx) = NetworkClient::new();
//!
//! // 连接到服务器
//! client.connect("ws://localhost:60051", "user123", "token").await?;
//!
//! // 监听连接事件
//! tokio::spawn(async move {
//!     while let Some(event) = connection_rx.recv().await {
//!         match event {
//!             ConnectionEvent::Connected => println!("Connected"),
//!             ConnectionEvent::Disconnected => println!("Disconnected"),
//!             ConnectionEvent::Error(e) => println!("Error: {}", e),
//!         }
//!     }
//! });
//!
//! // 监听消息
//! tokio::spawn(async move {
//!     while let Some(msg) = message_rx.recv().await {
//!         match msg {
//!             NetworkMessage::Received(frame) => {
//!                 // 处理收到的消息
//!             }
//!             NetworkMessage::CustomPushData { data_type, payload, metadata } => {
//!                 // 处理自定义推送数据
//!             }
//!             _ => {}
//!         }
//!     }
//! });
//! ```

// 子模块
mod types;
mod parser;
mod packet;
mod router;
mod listener;
mod client;

// 导出公共 API
pub use types::{NetworkMessage, ConnectionEvent};
pub use client::NetworkClient;
pub use router::MessageRouter;
