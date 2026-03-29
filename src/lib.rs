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
//! 各接入层（Tauri / FFI / 其他）可将 `content` 字节通过 `decode_content_bytes` + `decoded_content_to_elem`
//! 转为可序列化的 `Elem`（camelCase、`contentType` 标签），便于 JSON 等序列化供前端使用。
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
//!                    API (MessageApi / ConversationApi)
//!                                 │
//!                        Command / Query
//!                                 │
//!                             EventBus
//!                    ┌────────────┼────────────┐
//!                    │            │            │
//!              Connection      Sync        Message
//!                (FSM)       Engine       Engine
//!                    │            │            │
//!                    └────────────┼────────────┘
//!                                 │
//!                          Repository
//!                                 │
//!                            Storage
//!                                 │
//!              Network Pipeline (Decode → Router → EventBus)
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
//! client.message().add_reaction("msg_id", "👍").await?;
//! ```

pub mod client;
pub mod conversation;
pub mod core;
pub mod domain;
pub mod error;
pub mod event;
pub mod fsm;
pub mod middleware;
pub mod model;
pub mod reliable_queue;
pub mod types;
pub mod util;

// 与 flare-orchestrator 对齐的分层入口
pub mod application;
pub mod config;
pub mod infrastructure;

/// 与旧路径 `crate::lifecycle::*` 对齐：配置在 [`client::lifecycle`]；路径工具源实现在 [`util::paths`]（经 lifecycle 再导出）；SQLite 仓储在 [`util::sqlite_store`]（`lifecycle-sqlite`）。
pub mod lifecycle {
    pub use crate::client::lifecycle::*;
}

// 基础设施层对外别名，保持现有 crate::store / transport / protocol 路径可用
pub use crate::infrastructure::persistence as store;
pub use crate::infrastructure::protocol;
pub use crate::infrastructure::transport;

/// 错误类型与 Result 根级导出（与 flare-core 一致，便于 bindings 等使用）
pub use error::{ErrorCode, FlareError, Result, from_rpc_status};
/// 强类型 ID（防止 user_id / conversation_id 混用）
pub use types::{ConversationId, UserId};

/// 常用类型预导出
pub mod prelude {
    // client（含 Facade：MessageApi / ConversationApi / MessageBuildApi）
    pub use crate::client::{
        ConversationApi, IMClient, IMClientBuilder, MessageApi, MessageBuildApi, SdkConfig,
        SdkConfigBuilder,
    };
    pub use crate::core::SdkState;
    pub use crate::error::{ErrorCode, FlareError, Result, from_rpc_status};
    pub use crate::types::{ConversationId, UserId};

    // event
    pub use crate::event::{ConversationEvent, MessageEvent};
    pub use crate::event::{EventBus, EventReceiver, SdkEvent, Subscription};

    // store
    pub use crate::store::{ConversationStore, MessageStore, StoreProvider, SyncCursorStore};

    // sync（任务抽象与 SyncManager 位于 core::sync）
    pub use crate::core::{
        SyncContext, SyncMode, SyncPhase, SyncProgress, SyncTask, SyncTaskResult,
    };
    pub use crate::fsm::SyncState;

    // middleware
    pub use crate::middleware::{EventInterceptor, MessageInterceptor};

    // 会话 ID 生成（与 flare-core 一致，供上层创建会话使用）
    pub use crate::conversation::generate_single_chat_conversation_id;

    // protocol
    pub use crate::protocol::{Codec, ProtobufCodec};

    // content builder / decoder / message_elem / message builder
    pub use crate::model::{BuiltContent, ContentBuilder, MessageBuilder};
    pub use crate::model::{DecodedContent, decode_content, decode_content_bytes};
    pub use crate::model::{Elem, decoded_content_to_elem};

    // frequently used proto types
    pub use crate::model::message::{
        ConversationType, DeleteScope, DeleteType, MarkType, Message, MessageSource, MessageStatus,
        MessageType, ReactionAction,
    };

    // 领域视图（含昵称、头像）
    pub use crate::domain::UserProfile;
    pub use crate::model::{Conversation, IMMessage};
}
