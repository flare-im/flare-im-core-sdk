//! 会话查询处理器（Session Query Handler）
//!
//! 职责：处理会话（登录状态）相关的读操作

use crate::application::fsm::FsmManager;
use crate::application::queries::*;
use std::sync::Arc;

/// 会话查询处理器
pub struct SessionQueryHandler {
    fsm: Arc<FsmManager>,
}

impl SessionQueryHandler {
    pub fn new(fsm: Arc<FsmManager>) -> Self {
        Self { fsm }
    }
    
    /// 处理查询会话状态
    pub async fn handle_session_state(&self, _query: GetSessionStateQuery) -> anyhow::Result<crate::domain::session::SessionState> {
        Ok(self.fsm.session_state().await)
    }
    
    /// 处理查询连接状态
    pub async fn handle_connection_state(&self, _query: GetConnectionStateQuery) -> anyhow::Result<crate::domain::connection::ConnectionState> {
        Ok(self.fsm.connection_state().await)
    }
    
    /// 处理查询同步状态
    pub async fn handle_sync_state(&self, _query: GetSyncStateQuery) -> anyhow::Result<crate::domain::sync::SyncState> {
        Ok(self.fsm.sync_state().await)
    }
}
