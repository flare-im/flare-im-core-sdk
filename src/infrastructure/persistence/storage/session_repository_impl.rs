//! 会话仓储实现
//!
//! 实现 domain::session::repository::SessionRepository 接口

use crate::domain::message::model::SessionId;
use crate::domain::session::model::Session;
use crate::domain::session::repository::SessionRepository;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;

/// 会话仓储实现
pub struct SessionRepositoryImpl {
    /// 存储后端
    storage: Arc<dyn crate::infrastructure::storage::StorageBackend>,
}

impl SessionRepositoryImpl {
    pub fn new(storage: Arc<dyn crate::infrastructure::storage::StorageBackend>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl SessionRepository for SessionRepositoryImpl {
    async fn save(&self, session: &Session) -> Result<()> {
        // 转换为 ProtoSessionSummary
        let proto_summary = session.to_proto();

        // 转换为 SessionSummary（domain 类型）
        let domain_summary = crate::domain::session::SessionSummary::from(proto_summary);

        // 保存到存储
        self.storage
            .save_session(&domain_summary)
            .await
            .context("Failed to save session")
    }

    async fn find_by_id(&self, id: &SessionId) -> Result<Option<Session>> {
        let proto_summary = self
            .storage
            .get_session(id.as_str())
            .await
            .context("Failed to get session")?;

        match proto_summary {
            Some(summary) => {
                // summary 已经是 SessionSummary (domain 类型)，需要转换为 SessionSummaryProto
                // 然后通过 Session::from_proto 转换为 Session
                let proto_summary = summary.to_proto();
                Ok(Some(Session::from_proto(proto_summary)?))
            }
            None => Ok(None),
        }
    }

    async fn find_all(&self, limit: Option<usize>) -> Result<Vec<Session>> {
        use crate::infrastructure::storage::SessionFilter;
        let filter = SessionFilter::default();
        let proto_summaries = self
            .storage
            .get_sessions(filter)
            .await
            .context("Failed to get sessions")?;

        let mut sessions = Vec::new();
        for proto_summary in proto_summaries {
            // proto_summary 是 SessionSummary (domain 类型)，需要转换为 SessionSummaryProto
            let proto_summary_proto = proto_summary.to_proto();
            match Session::from_proto(proto_summary_proto) {
                Ok(session) => sessions.push(session),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to convert proto session to domain session");
                }
            }
        }

        Ok(sessions)
    }

    async fn delete(&self, id: &SessionId) -> Result<()> {
        self.storage
            .delete_session(id.as_str())
            .await
            .context("Failed to delete session")
    }
}
