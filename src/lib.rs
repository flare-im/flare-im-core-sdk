//! # Flare IM Core SDK
//!
//! 生产级事件流驱动的跨平台 IM 客户端 SDK。
//!
//! ## 设计原则
//!
//! **核心只做消息和会话**，其他能力通过扩展机制注入：
//! - `SyncTask` — 自定义同步任务（联系人、群列表等）
//! - `MessageInterceptor` — 消息拦截（端到端加密、内容过滤等）
//! - `EventInterceptor` — 事件拦截（日志、审计等）
//! - `Extension` 事件 — 非核心领域推送（在线状态、通话信令、自定义业务等）
//! - `CustomPush` — 服务端自定义推送透传
//!
//! ## 消息内容类型
//!
//! 支持 `message_content.proto` 定义的全部 26 种消息类型：
//! - 基础 (1-15): Text / Image / Video / Audio / File / Location / Card / Sticker / Emoji / Gif / Quote / LinkCard / Forward / Thread / MiniProgram
//! - 富媒体 (30-32): RichText / Markdown / ImageGroup
//! - 系统 (60-61): System / Notification
//! - 业务 (80-83): Vote / Task / Schedule / Announcement
//! - 自定义 (100): Custom
//! - 平台 (111-115): Placeholder (E2E / DecryptFailed / External / Imported / Migration)
//!
//! ## 消息操作
//!
//! 覆盖 `event.proto` 定义的全部事件操作 SDK 全流程：
//! - 发送 → 拦截器 → 网络发送 → 本地存储 → SendAck
//! - 撤回 → 网络发送 → 本地状态更新 → Recalled 事件
//! - 编辑 → 网络发送 → 本地内容更新 → Edited 事件
//! - 删除 → 网络发送 → 本地删除 → Deleted 事件
//! - 已读 → 网络发送 → ReadReceipt 事件
//! - 正在输入 → 网络发送 → Typing 事件
//! - 表情反应 → 网络发送 → ReactionUpdated 事件
//! - 置顶/取消 → 网络发送 → Pinned/Unpinned 事件
//! - 标记/取消 → 网络发送 → Marked/Unmarked 事件
//!
//! ## 架构概览
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       IMClient                              │
//! │  ┌──────────┬────────────────┐                              │
//! │  │MessageApi│ConversationApi │  ← 核心 API                  │
//! │  └────┬─────┴───────┬────────┘                              │
//! │       │ Command/Query│                                      │
//! │  ┌────▼─────────────▼──────────────────────────────────┐    │
//! │  │                  SdkEngine                           │    │
//! │  │  ┌──────────┐ ┌──────────┐ ┌──────────────────────┐ │    │
//! │  │  │EventBus  │ │Dispatcher│ │SyncManager           │ │    │
//! │  │  └──────────┘ └────┬─────┘ │ + custom SyncTasks   │ │    │
//! │  │                    │       └──────────────────────┘ │    │
//! │  │  ┌─────────────────▼─────────────────────────────┐  │    │
//! │  │  │  MiddlewareChain (interceptors)               │  │    │
//! │  │  └───────────────────────────────────────────────┘  │    │
//! │  │  ┌──────────────┐ ┌────────────┐ ┌──────────────┐  │    │
//! │  │  │SocketTransport│ │PacketSender│ │Router (store)│  │    │
//! │  │  └────┬──────────┘ └────────────┘ └──────────────┘  │    │
//! │  └───────┼─────────────────────────────────────────────┘    │
//! │          │                                                   │
//! │     FlareClient (WebSocket / QUIC)                          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 快速上手
//!
//! ```ignore
//! use flare_im_core_sdk::prelude::*;
//!
//! let client = IMClient::builder()
//!     .config(SdkConfig::new("wss://im.example.com"))
//!     .stores(stores)
//!     .build();
//!
//! client.connect("user_123", "jwt_token").await?;
//!
//! // 构建并发送文本消息
//! let content = ContentBuilder::text("Hello @All!")
//!     .mention_all(6, 4)
//!     .build();
//! let msg = MessageBuilder::new("conv_id", "user_123")
//!     .content(content)
//!     .build()?;
//! client.message().send(msg).await?;
//!
//! // 监听推送 + 解码内容
//! let _sub = client.on_message(|msg| {
//!     if let Ok(decoded) = decode_content(msg) {
//!         println!("{}", decoded.text_preview());
//!     }
//! });
//!
//! // 消息操作
//! client.message().typing("conv_id", "user_123", true).await?;
//! client.message().mark("conv_id", "msg_id", "user_123", MarkType::Important).await?;
//! client.message().pin("conv_id", "msg_id", "user_123").await?;
//! client.message().add_reaction("conv_id", "msg_id", "user_123", "👍").await?;
//! ```

pub mod error;
pub mod util;
pub mod model;
pub mod protocol;
pub mod store;
pub mod event;
pub mod core;
pub mod transport;
pub mod handler;
pub mod sync;
pub mod command;
pub mod query;
pub mod middleware;
pub mod api;
pub mod client;
pub mod conversation;

/// 常用类型预导出
pub mod prelude {
    // client
    pub use crate::client::{IMClient, IMClientBuilder, SdkConfig, SdkConfigBuilder};
    pub use crate::error::{SdkError, Result};
    pub use crate::core::SdkState;

    // event
    pub use crate::event::{SdkEvent, EventBus, EventReceiver, Subscription};
    pub use crate::event::{MessageEvent, ConversationEvent};

    // store
    pub use crate::store::{
        MessageStore, ConversationStore, SyncCursorStore, StoreProvider,
    };

    // sync
    pub use crate::sync::{SyncTask, SyncCompletion, SyncMode, SyncPhase, SyncContext};

    // middleware
    pub use crate::middleware::{MessageInterceptor, EventInterceptor};

    // api
    pub use crate::api::{MessageApi, ConversationApi};

    // 会话 ID 生成（与 flare-core 一致，供上层创建会话使用）
    pub use crate::conversation::generate_single_chat_conversation_id;

    // protocol
    pub use crate::protocol::{Codec, ProtobufCodec};

    // content builder / decoder / message builder
    pub use crate::model::{ContentBuilder, BuiltContent, MessageBuilder};
    pub use crate::model::{DecodedContent, decode_content, decode_content_bytes};

    // frequently used proto types
    pub use crate::model::message::{
        Message, MessageType, MessageStatus, MessageSource, ConversationType,
        MarkType, ReactionAction, DeleteType, DeleteScope,
    };
}
