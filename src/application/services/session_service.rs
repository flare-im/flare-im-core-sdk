//! 会话应用服务
//!
//! 编排会话相关的业务逻辑

use crate::application::handlers::{SessionCommandHandler, SessionQueryHandler};
use crate::domain::SessionId;
use crate::domain::session::SessionSummary;
use anyhow::Result;
use std::sync::Arc;

/// 会话应用服务
///
/// 编排会话相关的业务逻辑
pub struct SessionService {
    command_handler: Arc<SessionCommandHandler>,
    query_handler: Arc<SessionQueryHandler>,
}

impl SessionService {
    pub fn new(
        command_handler: Arc<SessionCommandHandler>,
        query_handler: Arc<SessionQueryHandler>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }

    /// 创建会话
    pub async fn create_session(
        &self,
        session_id: Option<SessionId>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Vec<String>,
    ) -> Result<SessionId> {
        use crate::application::commands::session::CreateSessionCommand;
        self.command_handler
            .handle_create_session(CreateSessionCommand {
                session_id,
                session_type,
                business_type,
                display_name,
                participants,
            })
            .await
    }

    /// 获取会话列表
    pub async fn get_sessions(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
    ) -> Result<Vec<SessionSummary>> {
        use crate::application::queries::session::GetSessionsQuery;
        self.query_handler
            .handle_get_sessions(GetSessionsQuery { filter })
            .await
    }
}
