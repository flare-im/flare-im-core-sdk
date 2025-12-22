//! 事件订阅器 Trait 定义（领域接口）
//!
//! 定义事件订阅的领域接口，不依赖任何基础设施实现
//!
//! ## DDD 设计原则
//!
//! 1. **领域接口**: 这些 trait 定义了领域模型的事件订阅契约
//! 2. **无依赖**: 不依赖任何基础设施框架（tokio、serde 等仅用于类型定义）
//! 3. **类型安全**: 每种事件类型都有专门的订阅器 trait，编译期保证类型安全
//!
//! ## 使用示例
//!
//! ```rust
//! use flare_im_core_sdk::domain::event::subscribers::*;
//!
//! struct MyMessageSubscriber;
//!
//! #[async_trait::async_trait]
//! impl MessageEventSubscriber for MyMessageSubscriber {
//!     async fn on_message_delivered(&self, event: &MessageDelivered) -> anyhow::Result<()> {
//!         println!("收到消息: {}", event.message_id);
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use crate::domain::event::{
    ConnectionConnected, ConnectionDisconnected, ConnectionReconnecting, 
    ConnectionReconnected, ConnectionConnectFailed,
    SessionLoggedIn, SessionLoggedOut, SessionExpired, SessionTokenRefreshed,
    MessageCreated, MessageSent, MessageSendFailed, MessageDelivered, MessageRead,
    MessageRecalled, MessageEdited, MessageDeleted, MessageReactionAdded,
    MessageReactionRemoved, MessagePinned, MessageUnpinned, MessageFavorited,
    MessageUnfavorited, MessageMarked, MessageUnmarked, MessageForwarded, MessageReplied,
    SyncBootstrapStarted, SyncBootstrapCompleted, SyncBootstrapFailed,
    SyncAsyncStarted, SyncAsyncCompleted, SyncAsyncFailed, SyncProgressUpdated,
    ConversationCreated, ConversationUnreadUpdated, ConversationLastMessageUpdated,
    ConversationMarkedAsRead, ConversationDraftUpdated, ConversationHidden,
    ConversationAllHidden, ConversationDeleted, ConversationMessagesCleared,
    ConversationUpdated, ConversationMuted, ConversationUnmuted,
    ConversationPinned, ConversationUnpinned, ConversationArchived,
    ConversationUnarchived, ConversationInputStateUpdated, ConversationInputStateCleared,
};

// ============================================================================
// Connection 事件订阅器（领域接口）
// ============================================================================

/// Connection 事件订阅器
///
/// 处理连接相关的所有事件，包括连接、断开、重连等
#[async_trait]
pub trait ConnectionEventSubscriber: Send + Sync {
    /// 连接已建立
    async fn on_connected(&self, event: &ConnectionConnected) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 连接已断开
    async fn on_disconnected(&self, event: &ConnectionDisconnected) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 正在重连
    async fn on_reconnecting(&self, event: &ConnectionReconnecting) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 重连成功
    async fn on_reconnected(&self, event: &ConnectionReconnected) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 连接失败
    async fn on_connect_failed(&self, event: &ConnectionConnectFailed) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }
}

// ============================================================================
// Session 事件订阅器（领域接口）
// ============================================================================

/// Session 事件订阅器
///
/// 处理会话相关的所有事件，包括登录、登出、Token 刷新等
#[async_trait]
pub trait SessionEventSubscriber: Send + Sync {
    /// 已登录
    async fn on_logged_in(&self, event: &SessionLoggedIn) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 已登出
    async fn on_logged_out(&self, event: &SessionLoggedOut) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已过期
    async fn on_expired(&self, event: &SessionExpired) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// Token 已刷新
    async fn on_token_refreshed(&self, event: &SessionTokenRefreshed) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }
}

// ============================================================================
// Message 事件订阅器（领域接口）
// ============================================================================

/// Message 事件订阅器
///
/// 处理消息相关的所有事件，包括发送、接收、已读、撤回等
#[async_trait]
pub trait MessageEventSubscriber: Send + Sync {
    /// 消息已创建
    async fn on_message_created(&self, event: &MessageCreated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已发送
    async fn on_message_sent(&self, event: &MessageSent) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息发送失败
    async fn on_message_send_failed(&self, event: &MessageSendFailed) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已送达
    async fn on_message_delivered(&self, event: &MessageDelivered) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已读
    async fn on_message_read(&self, event: &MessageRead) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已撤回
    async fn on_message_recalled(&self, event: &MessageRecalled) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已编辑
    async fn on_message_edited(&self, event: &MessageEdited) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已删除
    async fn on_message_deleted(&self, event: &MessageDeleted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息反应已添加
    async fn on_message_reaction_added(&self, event: &MessageReactionAdded) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息反应已移除
    async fn on_message_reaction_removed(&self, event: &MessageReactionRemoved) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已置顶
    async fn on_message_pinned(&self, event: &MessagePinned) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已取消置顶
    async fn on_message_unpinned(&self, event: &MessageUnpinned) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已收藏
    async fn on_message_favorited(&self, event: &MessageFavorited) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已取消收藏
    async fn on_message_unfavorited(&self, event: &MessageUnfavorited) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已标记
    async fn on_message_marked(&self, event: &MessageMarked) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已取消标记
    async fn on_message_unmarked(&self, event: &MessageUnmarked) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已转发
    async fn on_message_forwarded(&self, event: &MessageForwarded) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已回复
    async fn on_message_replied(&self, event: &MessageReplied) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }
}

// ============================================================================
// Conversation 事件订阅器（领域接口）
// ============================================================================

/// Conversation 事件订阅器
///
/// 处理会话相关的所有事件，包括创建、更新、未读数变化等
#[async_trait]
pub trait ConversationEventSubscriber: Send + Sync {
    /// 会话已创建
    async fn on_conversation_created(&self, event: &ConversationCreated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 未读数已更新
    async fn on_unread_updated(&self, event: &ConversationUnreadUpdated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 最后消息已更新
    async fn on_last_message_updated(&self, event: &ConversationLastMessageUpdated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已标记为已读
    async fn on_marked_as_read(&self, event: &ConversationMarkedAsRead) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 草稿已更新
    async fn on_draft_updated(&self, event: &ConversationDraftUpdated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已隐藏
    async fn on_hidden(&self, event: &ConversationHidden) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 所有会话已隐藏
    async fn on_all_hidden(&self, event: &ConversationAllHidden) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已删除
    async fn on_deleted(&self, event: &ConversationDeleted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 消息已清空
    async fn on_messages_cleared(&self, event: &ConversationMessagesCleared) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话信息已更新
    async fn on_updated(&self, event: &ConversationUpdated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已静音
    async fn on_muted(&self, event: &ConversationMuted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已取消静音
    async fn on_unmuted(&self, event: &ConversationUnmuted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已置顶
    async fn on_pinned(&self, event: &ConversationPinned) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已取消置顶
    async fn on_unpinned(&self, event: &ConversationUnpinned) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已归档
    async fn on_archived(&self, event: &ConversationArchived) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 会话已取消归档
    async fn on_unarchived(&self, event: &ConversationUnarchived) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 输入状态已更新
    async fn on_input_state_updated(&self, event: &ConversationInputStateUpdated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 输入状态已清除
    async fn on_input_state_cleared(&self, event: &ConversationInputStateCleared) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }
}

// ============================================================================
// Sync 事件订阅器（领域接口）
// ============================================================================

/// Sync 事件订阅器
///
/// 处理同步相关的所有事件，包括 Bootstrap Sync、Async Sync 等
#[async_trait]
pub trait SyncEventSubscriber: Send + Sync {
    /// Bootstrap Sync 已开始
    async fn on_bootstrap_started(&self, event: &SyncBootstrapStarted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// Bootstrap Sync 已完成
    async fn on_bootstrap_completed(&self, event: &SyncBootstrapCompleted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// Bootstrap Sync 已失败
    async fn on_bootstrap_failed(&self, event: &SyncBootstrapFailed) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// Async Sync 已开始
    async fn on_async_started(&self, event: &SyncAsyncStarted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// Async Sync 已完成
    async fn on_async_completed(&self, event: &SyncAsyncCompleted) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// Async Sync 已失败
    async fn on_async_failed(&self, event: &SyncAsyncFailed) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }

    /// 同步进度已更新
    async fn on_progress_updated(&self, event: &SyncProgressUpdated) -> anyhow::Result<()> {
        let _ = event;
        Ok(())
    }
}
