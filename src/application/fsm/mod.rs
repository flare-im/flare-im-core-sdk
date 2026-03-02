use std::sync::Arc;
use tokio::sync::RwLock;
use crate::domain::session::{Session, SessionState};
use crate::domain::connection::{Connection, ConnectionState};
use crate::domain::sync::{Sync, SyncState};
use crate::domain::message::Message;
use crate::domain::event::DomainEvent;
use crate::domain::repository::EventStore;

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
    
    pub async fn session_start_login(&self) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.start_login()?;
        
        let event = DomainEvent::new(
            "Session.LoggingIn",
            "session",
            session.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    pub async fn session_login_success(
        &self,
        user_id: String,
        token: String,
    ) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.login_success(user_id.clone(), token.clone())?;
        
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
    
    pub async fn session_logout(&self) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.logout()?;
        
        let event = DomainEvent::new(
            "Session.LoggedOut",
            "session",
            session.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    pub async fn session_expire(&self) -> anyhow::Result<()> {
        let mut session = self.session.write().await;
        session.expire()?;
        
        let event = DomainEvent::new(
            "Session.Expired",
            "session",
            session.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    pub async fn session_state(&self) -> SessionState {
        let session = self.session.read().await;
        session.state
    }
    
    pub async fn current_user_id(&self) -> Option<String> {
        let session = self.session.read().await;
        session.user_id.clone()
    }
    
    pub async fn current_token(&self) -> Option<String> {
        let session = self.session.read().await;
        session.token.clone()
    }
    
    pub async fn session_info(&self) -> (Option<String>, Option<String>) {
        let session = self.session.read().await;
        (session.user_id.clone(), session.token.clone())
    }
    
    pub async fn connection_start_connect(&self) -> anyhow::Result<()> {
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
    ///
    /// 断开连接时，自动重置 Sync 状态，允许重新连接后重新开始 Bootstrap Sync
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
        
        // 断开连接时，重置 Sync 状态，允许重新连接后重新开始 Bootstrap Sync
        // 注意：这里不重置游标，保留历史同步信息
        self.sync_reset().await?;
        
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
    
    /// 重置 Sync 状态（Sync FSM: Any -> Idle）
    ///
    /// 用于连接断开时重置同步状态，允许重新开始 Bootstrap Sync
    pub async fn sync_reset(&self) -> anyhow::Result<()> {
        let mut sync = self.sync.write().await;
        sync.reset();
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Sync.Reset",
            "sync",
            sync.version,
            serde_json::json!({}),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 开始 Bootstrap Sync（Sync FSM: Idle -> Bootstrapping）
    ///
    /// 如果连接已断开后重新连接，允许从 Ready 状态重新开始
    pub async fn sync_start_bootstrap(&self) -> anyhow::Result<()> {
        // 检查 Connection 状态
        let connection_state = self.connection_state().await;
        if connection_state != ConnectionState::Online {
            return Err(anyhow::anyhow!("Connection is not Online, cannot start bootstrap sync"));
        }
        
        let mut sync = self.sync.write().await;
        // 检查当前状态，如果是 Ready，允许重新开始（连接断开后重新连接的情况）
        let allow_from_ready = sync.state == crate::domain::sync::SyncState::Ready;
        sync.start_bootstrap(allow_from_ready)?;
        
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
            message.server_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
            message.version,
            serde_json::json!({
                "message_id": message.server_id.clone().unwrap_or_default(),
                "conversation_id": message.conversation_id.clone().unwrap_or_default(),
                "sender_id": message.sender_id,
                "is_retry": is_retry,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
    
    /// 消息发送成功（Message FSM: Sending -> Sent）
    pub async fn message_send_success(&self, message: &mut Message, seq: u64, server_msg_id: String) -> anyhow::Result<()> {
        message.send_success(seq,server_msg_id.clone())?;
        
        // 发布领域事件
        let event = DomainEvent::new(
            "Message.Sent",
            &server_msg_id,
            message.version,
            serde_json::json!({
                "server_msg_id": server_msg_id,
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
            message.server_id.as_ref().map(|s| s.as_str()).unwrap_or(""),
            message.version,
            serde_json::json!({
                "message_id": message.server_id.clone().unwrap_or_default(),
                "error": error,
            }),
        );
        self.event_store.append(event).await?;
        
        Ok(())
    }
}
