//! SdkContext
//!
//! 提供给扩展使用的核心 SDK 能力

use std::sync::Arc;
use crate::application::handlers::{CommandHandler, QueryHandler};
use crate::application::sync_coordinator::SyncCoordinator;
use crate::domain::repository::{EventStore, ReadStore};
use crate::infrastructure::event_bus::EventBus;
use crate::application::fsm::FsmManager;

/// SDK Context
///
/// 提供给扩展使用的核心 SDK 能力
/// 扩展可以通过 SdkContext 访问核心的登录、连接、存储等能力
#[derive(Clone)]
pub struct SdkContext {
    /// 命令总线（用于写操作）
    pub command_handler: Arc<CommandHandler>,
    
    /// 查询总线（用于读操作）
    pub query_handler: Arc<QueryHandler>,
    
    /// 事件总线（用于发布/订阅领域事件）
    pub event_bus: Arc<EventBus>,
    
    /// 事件存储（用于持久化领域事件）
    pub event_store: Arc<dyn EventStore>,
    
    /// 读存储（用于查询）
    pub read_store: Arc<dyn ReadStore>,
    
    /// 同步协调器（用于同步）
    pub sync_coordinator: Arc<SyncCoordinator>,
    
    /// FSM 管理器（用于状态管理）
    pub fsm: Arc<FsmManager>,
}

impl SdkContext {
    /// 创建新的 SdkContext
    pub fn new(
        command_handler: Arc<CommandHandler>,
        query_handler: Arc<QueryHandler>,
        event_bus: Arc<EventBus>,
        event_store: Arc<dyn EventStore>,
        read_store: Arc<dyn ReadStore>,
        sync_coordinator: Arc<SyncCoordinator>,
        fsm: Arc<FsmManager>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
            event_bus,
            event_store,
            read_store,
            sync_coordinator,
            fsm,
        }
    }
    
    /// 获取当前用户 ID（从 Session 获取）
    ///
    /// 对标微信、Telegram、飞书的用户状态获取
    pub async fn current_user_id(&self) -> Option<String> {
        self.fsm.current_user_id().await
    }
    
    /// 检查是否已登录
    ///
    /// 对标微信、Telegram、飞书的登录状态检查
    pub async fn is_logged_in(&self) -> bool {
        use crate::domain::session::SessionState;
        let session_state = self.fsm.session_state().await;
        matches!(session_state, SessionState::Active)
    }
    
    /// 检查连接是否在线
    ///
    /// 对标微信、Telegram、飞书的连接状态检查
    pub async fn is_connected(&self) -> bool {
        use crate::domain::connection::ConnectionState;
        let connection_state = self.fsm.connection_state().await;
        matches!(connection_state, ConnectionState::Online)
    }
}
