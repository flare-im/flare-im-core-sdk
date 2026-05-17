//! SDK 事件类型（按域分组，飞书级 IM 语义，便于内部扩展与跨语言绑定）
//!
//! 内部仅通过 Event 通信；对外暴露类型化回调 API（on_*），
//! 不暴露大 trait，便于 FFI / Swift / Kotlin / TypeScript 绑定。

use flare_proto::common::{
    CallSignalEvent, CustomEvent, MarkEvent, MessageDeleteEvent, MessageRecallEvent, PinEvent,
    PresenceEvent, ReadReceiptEvent, SendAck, TypingEvent, UnmarkEvent, UnpinEvent,
};

use crate::core::{SdkState, SyncRunContext};
use crate::fsm::SyncState;
use crate::model::IMMessage;

// ========== 域事件枚举（Domain events） ==========

/// 连接域事件：连接生命周期、状态变更、服务端错误、被踢下线、登录凭证过期
///
/// 与飞书级 IM 对齐：支持多端登录冲突（被踢）、Token 过期需重新登录等场景。
#[derive(Clone, Debug)]
pub enum ConnectionEvent {
    /// 已连接（含认证通过）
    Connected,
    /// 断开连接（含原因描述）
    Disconnected { reason: String },
    /// 连接状态变更（Disconnected / Connecting / Connected / Ready / Reconnecting）
    StateChanged { state: SdkState },
    /// 同步状态变更（Idle / Syncing / CatchingUp / Error）
    SyncStateChanged { state: SyncState },
    /// 服务端返回错误（code + message）
    ServerError { code: i32, message: String },
    /// 重连中（attempt 为当前第几次重试）
    Reconnecting { attempt: u32 },
    /// 账号已在其他设备/地点登录，当前设备被踢下线
    KickedOff { reason: String },
    /// 登录凭证已过期，需要重新登录（如刷新 Token 或跳转登录页）
    TokenExpired { message: String },
}

/// 消息域事件：收到新消息、发送成功/失败、撤回、正在输入、批量新消息
///
/// 与飞书级 IM 对齐：单条与批量新消息、发送回执、撤回、Typing 等。
/// SDK 内部仅使用 IMMessage 流转。
#[derive(Clone, Debug)]
pub enum MessageEvent {
    /// 收到一条新消息（单聊/群聊推送或同步拉取）
    Received { message: IMMessage },
    /// 新消息批量（同步或批量推送时一次下发多条，减少回调次数）
    ReceivedBatch { messages: Vec<IMMessage> },
    /// 消息发送成功（服务端回执）
    SendAck { ack: SendAck },
    /// 消息发送失败（client_msg_id + 原因）
    SendFailed {
        client_msg_id: String,
        reason: String,
    },
    /// 消息被撤回（会话 id + 撤回事件体）
    Recalled {
        conversation_id: String,
        event: MessageRecallEvent,
    },
    /// 正在输入（会话 id + Typing 事件体）
    Typing {
        conversation_id: String,
        event: TypingEvent,
    },
    /// 消息正文已更新（本地编辑确认或服务端推送/同步下发后，本地库已写入新 `content`）
    Edited {
        conversation_id: String,
        server_msg_id: String,
        /// 服务端编辑版本（`MessageEditEvent.edit_version`），无则 `None`
        edit_version: Option<i32>,
    },
    /// 消息反应已变化（添加/移除）
    ReactionChanged {
        conversation_id: String,
        server_msg_id: String,
        user_id: String,
        emoji: String,
        /// 与 proto `ReactionAction` 对齐：1=ADD, 2=REMOVE
        action: i32,
    },
    /// 消息被删除（服务端事件）
    Deleted {
        conversation_id: String,
        event: MessageDeleteEvent,
    },
    /// 已读回执（服务端事件）
    ReadReceipt {
        conversation_id: String,
        event: ReadReceiptEvent,
    },
    /// 消息被置顶
    Pinned {
        conversation_id: String,
        event: PinEvent,
    },
    /// 消息取消置顶
    Unpinned {
        conversation_id: String,
        event: UnpinEvent,
    },
    /// 消息被标记
    Marked {
        conversation_id: String,
        event: MarkEvent,
    },
    /// 消息取消标记
    Unmarked {
        conversation_id: String,
        event: UnmarkEvent,
    },
    /// 在线状态事件（presence）
    PresenceChanged {
        conversation_id: String,
        event: PresenceEvent,
    },
    /// 通话信令事件（call_signal）
    CallSignal {
        conversation_id: String,
        event: CallSignalEvent,
    },
    /// 自定义领域事件（custom）
    Custom {
        conversation_id: String,
        event: CustomEvent,
    },
}

/// 会话域事件：新会话、会话信息变更、未读数变化、会话删除、列表同步完成
///
/// 与飞书级 IM 对齐：会话创建、更新、未读变化、删除及全量同步完成。
#[derive(Clone, Debug)]
pub enum ConversationEvent {
    /// 会话列表全量同步完成（拉取到的会话 id 列表）
    Synced { conversation_ids: Vec<String> },
    /// 新会话（首次出现或服务端下发新建）
    Created { conversation_id: String },
    /// 会话信息变更（标题、置顶、未读等除未读数外的变更也可走此事件）
    Updated { conversation_id: String },
    /// 会话未读数量变化（本地已读或服务端推送未读更新）
    UnreadCountChanged {
        conversation_id: String,
        unread_count: u32,
    },
    /// 会话被删除
    Deleted { conversation_id: String },
}

/// 同步域事件：状态、阶段、进度、任务完成/失败
///
/// 命名为 `SyncNotify`（非 `Sync`），避免与 Rust `std::marker::Sync` 及 wire 层 `flare_proto::common::Sync` 混淆。
#[derive(Clone, Debug)]
pub enum SyncNotify {
    StateChanged {
        run: SyncRunContext,
        state: SyncState,
    },
    Started {
        run: SyncRunContext,
    },
    Finished {
        run: SyncRunContext,
        phase: SyncPhase,
    },
    Failed {
        run: SyncRunContext,
        task: String,
        message: String,
    },
    Progress {
        run: SyncRunContext,
        task: String,
        progress: f32,
        detail: String,
    },
    TaskCompleted {
        run: SyncRunContext,
        task: String,
    },
}

impl SyncNotify {
    pub fn is_user_visible(&self) -> bool {
        match self {
            Self::StateChanged { run, .. }
            | Self::Started { run }
            | Self::Finished { run, .. }
            | Self::Failed { run, .. }
            | Self::Progress { run, .. }
            | Self::TaskCompleted { run, .. } => run.visibility.is_user_visible(),
        }
    }
}

/// 扩展域事件：业务自定义推送
#[derive(Clone, Debug)]
pub struct ExtensionEvent {
    pub source: String,
    pub event_type: String,
    pub payload: Vec<u8>,
}

/// 同步阶段（用于 SyncFinished）
#[derive(Clone, Debug)]
pub enum SyncPhase {
    Init,
    Background,
}

// ========== 顶层 SDK 事件（内部总线统一入口） ==========

/// 顶层 SDK 事件：按域聚合，内部总线与跨语言绑定均使用此枚举
#[derive(Clone, Debug)]
pub enum SdkEvent {
    Connection(ConnectionEvent),
    Message(MessageEvent),
    Conversation(ConversationEvent),
    Sync(SyncNotify),
    Extension(ExtensionEvent),
}
