//! 会话领域服务接口和实现
//!
//! 封装复杂的业务逻辑，不依赖基础设施

use crate::domain::message::model::{SessionId, UserId};
use crate::domain::session::SessionSummary;
use crate::domain::session::model::Session;
use anyhow::{Context, Result};
use async_trait::async_trait;

/// 会话领域服务接口
///
/// 封装会话相关的复杂业务逻辑
#[async_trait]
pub trait SessionDomainService: Send + Sync {
    /// 创建会话
    async fn create_session(
        &self,
        id: Option<SessionId>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Vec<String>,
    ) -> Result<Session>;

    /// 验证会话
    async fn validate_session(&self, session: &Session) -> Result<()>;

    /// 生成会话 ID
    async fn generate_session_id(
        &self,
        session_type: &str,
        business_type: &str,
        target_id: &str,
    ) -> Result<SessionId> {
        let session_id = format!("{}:{}:{}", session_type, business_type, target_id);
        Ok(SessionId::new(session_id))
    }

    /// 计算未读数
    ///
    /// # 参数
    /// - `session`: 会话摘要
    /// - `last_read_seq`: 最后已读序列号
    ///
    /// # 返回
    /// - `u32`: 未读数
    fn calculate_unread_count(&self, session: &SessionSummary, last_read_seq: u64) -> u32 {
        if session.max_seq > last_read_seq {
            (session.max_seq - last_read_seq) as u32
        } else {
            0
        }
    }

    /// 增加未读数
    ///
    /// # 参数
    /// - `current_count`: 当前未读数
    /// - `increment`: 增量（默认 1）
    ///
    /// # 返回
    /// - `u32`: 新的未读数
    fn increment_unread_count(&self, current_count: u32, increment: u32) -> u32 {
        current_count.saturating_add(increment)
    }

    /// 重置未读数
    ///
    /// # 返回
    /// - `0`: 重置后的未读数
    fn reset_unread_count(&self) -> u32 {
        0
    }

    /// 判断是否需要同步（基于未读数和时间）
    ///
    /// # 参数
    /// - `unread_count`: 未读数
    /// - `last_sync_time`: 最后同步时间（毫秒时间戳）
    /// - `sync_threshold`: 同步阈值（未读数超过此值需要同步）
    /// - `time_threshold`: 时间阈值（毫秒，超过此时间需要同步）
    ///
    /// # 返回
    /// - `bool`: 是否需要同步
    fn should_sync(
        &self,
        unread_count: u32,
        last_sync_time: Option<i64>,
        sync_threshold: u32,
        time_threshold: i64,
    ) -> bool {
        if unread_count > sync_threshold {
            return true;
        }

        if let Some(last_sync) = last_sync_time {
            let now = chrono::Utc::now().timestamp_millis();
            if now - last_sync > time_threshold {
                return true;
            }
        }

        false
    }
}

/// 会话领域服务实现
pub struct SessionDomainServiceImpl;

impl SessionDomainServiceImpl {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SessionDomainService for SessionDomainServiceImpl {
    async fn create_session(
        &self,
        id: Option<SessionId>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Vec<String>,
    ) -> Result<Session> {
        // 如果没有提供 ID，生成一个
        let session_id = id.unwrap_or_else(|| {
            // TODO: 使用会话 ID 生成器
            SessionId::new(format!("session_{}", uuid::Uuid::new_v4()))
        });

        // 创建会话聚合根
        let session = Session::new(
            session_id,
            session_type,
            business_type,
            display_name,
            participants,
        );

        // 验证会话
        self.validate_session(&session).await?;

        Ok(session)
    }

    async fn validate_session(&self, session: &Session) -> Result<()> {
        session.validate()
    }

    fn calculate_unread_count(&self, session: &SessionSummary, last_read_seq: u64) -> u32 {
        if session.max_seq > last_read_seq {
            (session.max_seq - last_read_seq) as u32
        } else {
            0
        }
    }

    fn increment_unread_count(&self, current_count: u32, increment: u32) -> u32 {
        current_count.saturating_add(increment)
    }

    fn reset_unread_count(&self) -> u32 {
        0
    }

    fn should_sync(
        &self,
        unread_count: u32,
        last_sync_time: Option<i64>,
        sync_threshold: u32,
        time_threshold: i64,
    ) -> bool {
        if unread_count > sync_threshold {
            return true;
        }

        if let Some(last_sync) = last_sync_time {
            let now = chrono::Utc::now().timestamp_millis();
            if now - last_sync > time_threshold {
                return true;
            }
        }

        false
    }
}
