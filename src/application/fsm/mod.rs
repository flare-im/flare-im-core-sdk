//! FSM 状态机系统
//!
//! 所有状态变化必须经过 FSM
//! FSM 位于 Application Layer

use std::sync::Arc;
use tokio::sync::RwLock;
use crate::domain::session::{Session, SessionState};
use crate::domain::connection::{Connection, ConnectionState};
use crate::domain::sync::{Sync, SyncState};
use crate::domain::message::{Message, MessageState};
use crate::domain::event::DomainEvent;
use crate::domain::repository::EventStore;

/// FSM 管理器
pub struct FsmManager {
    session: Arc<RwLock<Session>>,
    connection: Arc<RwLock<Connection>>,
    sync: Arc<RwLock<Sync>>,
    event_store: Arc<dyn EventStore>,
}

impl FsmManager {
    pub fn new(
        session: Session,
        connection: Connection,
        sync: Sync,
        event_store: Arc<dyn EventStore>,
    ) -> Self {
        Self {
            session: Arc::new(RwLock::new(session)),
            connection: Arc::new(RwLock::new(connection)),
            sync: Arc::new(RwLock::new(sync)),
            event_store,
        }
    }
    
    // ============================================================================
    // Session FSM
    // ============================================================================
    
    /// 开始登录（Session FSM: Idle -> LoggingIn）
    pub async fn session_start_login(&self) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.start_login()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Session.LoggingIn",
            "session",
            session.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 登录成功（Session FSM: LoggingIn -> Active）
    pub async fn session_login_success(
        &self,
        user_id: String,
        token: String,
    ) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.login_success(user_id.clone(), token.clone())?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Session.LoggedIn",
            "session",
            session.version,
            serde_json::json!({
                "user_id": user_id,
                "token": token,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 登出（Session FSM: Active -> Idle）
    pub async fn session_logout(&self) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.logout()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Session.LoggedOut",
            "session",
            session.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 标记过期（Session FSM: Active -> Expired）
    pub async fn session_expire(&self) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.expire()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Session.Expired",
            "session",
            session.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 获取 Session 状态
    pub async fn session_state(&self) -> SessionState {
        let session = self.session.read().await;
        session.state
    }
    
    /// 获取当前用户ID（如果已登录）
    pub async fn current_user_id(&self) -> Option<String> {
        let session = self.session.read().await;
        session.user_id.clone()
    }
    
    /// 获取当前Token（如果已登录）
    pub async fn current_token(&self) -> Option<String> {
        let session = self.session.read().await;
        session.token.clone()
    }
    
    /// 获取Session信息（user_id和token）
    pub async fn session_info(&self) -> (Option<String>, Option<String>) {
        let session = self.session.read().await;
        (session.user_id.clone(), session.token.clone())
    }
    
    // ============================================================================
    // Connection FSM
    // ============================================================================
    
    /// 开始连接（Connection FSM: Disconnected -> Connecting）
    pub async fn connection_start_connect(&self) -> anyhow::Result<()> {
        // 检查 Session 状态
        let session_state = self.session_state().await;
        if session_state != SessionState::Active {
            return Err(anyhow::anyhow!("Session is not Active, cannot connect"));
        }
        
        let mut connection = self.connection.write().await;
        connection.start_connect()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Connection.Connecting",
            "connection",
            connection.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 连接成功（Connection FSM: Connecting -> Online）
    pub async fn connection_connect_success(&self, connection_id: String) -> anyhow::Result<()> {
        let mut connection = self.connection.write().await;
        connection.connect_success(connection_id.clone())?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Connection.Connected",
            "connection",
            connection.version,
            serde_json::json!({
                "connection_id": connection_id,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 断开连接（Connection FSM: Online -> Disconnected）
    pub async fn connection_disconnect(&self) -> anyhow::Result<()> {
        let mut connection = self.connection.write().await;
        connection.disconnect()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Connection.Disconnected",
            "connection",
            connection.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 开始重连（Connection FSM: Online -> Reconnecting）
    pub async fn connection_start_reconnect(&self) -> anyhow::Result<()> {
        let mut connection = self.connection.write().await;
        connection.start_reconnect()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Connection.Reconnecting",
            "connection",
            connection.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 获取 Connection 状态
    pub async fn connection_state(&self) -> ConnectionState {
        let connection = self.connection.read().await;
        connection.state
    }
    
    // ============================================================================
    // Sync FSM
    // ============================================================================
    
    /// 开始 Bootstrap Sync（Sync FSM: Idle -> Bootstrapping）
    pub async fn sync_start_bootstrap(&self) -> anyhow::Result<()> {
        // 检查 Connection 状态
        let connection_state = self.connection_state().await;
        if connection_state != ConnectionState::Online {
            return Err(anyhow::anyhow!("Connection is not Online, cannot start bootstrap sync"));
        }
        
        let mut sync = self.sync.write().await;
        sync.start_bootstrap()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.BootstrapStarted",
            "sync",
            sync.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// Bootstrap Sync 完成（Sync FSM: Bootstrapping -> Ready）
    pub async fn sync_bootstrap_completed(&self, cursor: String) -> anyhow::Result<()> {
        let mut sync = self.sync.write().await;
        sync.bootstrap_completed(cursor.clone())?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.BootstrapCompleted",
            "sync",
            sync.version,
            serde_json::json!({
                "cursor": cursor,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// Bootstrap Sync 失败（Sync FSM: Bootstrapping -> Idle）
    pub async fn sync_bootstrap_failed(&self) -> anyhow::Result<()> {
        let mut sync = self.sync.write().await;
        sync.bootstrap_failed()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.BootstrapFailed",
            "sync",
            sync.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 开始 Async Sync（Sync FSM: Ready -> Syncing）
    pub async fn sync_start_async(&self, sync_type: String) -> anyhow::Result<()> {
        let mut sync = self.sync.write().await;
        sync.start_async(sync_type.clone())?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.AsyncStarted",
            "sync",
            sync.version,
            serde_json::json!({
                "sync_type": sync_type,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// Async Sync 完成（Sync FSM: Syncing -> Ready）
    pub async fn sync_async_completed(&self, sync_type: String, cursor: String) -> anyhow::Result<()> {
        let mut sync = self.sync.write().await;
        sync.async_completed(sync_type.clone(), cursor.clone())?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.AsyncCompleted",
            "sync",
            sync.version,
            serde_json::json!({
                "sync_type": sync_type,
                "cursor": cursor,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// Async Sync 失败（Sync FSM: Syncing -> Ready）
    pub async fn sync_async_failed(&self) -> anyhow::Result<()> {
        let mut sync = self.sync.write().await;
        sync.async_failed()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.AsyncFailed",
            "sync",
            sync.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 获取 Sync 状态
    pub async fn sync_state(&self) -> SyncState {
        let sync = self.sync.read().await;
        sync.state
    }
    
    // ============================================================================
    // Message FSM
    // ============================================================================
    
    /// 开始发送消息（Message FSM: Created -> Sent）
    ///
    /// # 参数
    /// * `message` - 要发送的消息
    /// * `is_retry` - 是否是重试发送（如果为 true，允许从 Sent/Failed 状态重新发送）
    pub async fn message_start_sending(&self, message: &mut Message, is_retry: bool) -> anyhow::Result<()> {
        // 检查 Sync 状态（重试时也检查，确保同步状态正常）
        let sync_state = self.sync_state().await;
        if sync_state != SyncState::Ready {
            return Err(anyhow::anyhow!("Sync is not Ready, cannot send message"));
        }
        
        // 调用消息的 start_sending，支持重试
        message.start_sending(is_retry)?;
        
        // 发布领域事件
        let event_type = if is_retry { "Message.Retry" } else { "Message.Created" };
        let event = DomainEvent::new(
            event_type,
            &message.id,
            message.version,
            serde_json::json!({
                "message_id": message.id,
                "conversation_id": message.conversation_id,
                "sender_id": message.sender_id,
                "is_retry": is_retry,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 消息发送成功（Message FSM: Sending -> Sent）
    pub async fn message_send_success(&self, message: &mut Message, seq: u64) -> anyhow::Result<()> {
        message.send_success(seq)?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Message.Sent",
            &message.id,
            message.version,
            serde_json::json!({
                "message_id": message.id,
                "seq": seq,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 消息发送失败（Message FSM: Sending -> Failed）
    pub async fn message_send_failed(&self, message: &mut Message, error: String) -> anyhow::Result<()> {
        message.mark_failed()?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Message.SendFailed",
            &message.id,
            message.version,
            serde_json::json!({
                "message_id": message.id,
                "error": error,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
}
