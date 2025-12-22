//! Session 聚合根
//!
//! 职责：管理登录态和 Token

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Session 聚合根
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// 用户ID
    pub user_id: Option<String>,
    
    /// Token
    pub token: Option<String>,
    
    /// 设备ID
    pub device_id: String,
    
    /// 当前状态
    pub state: SessionState,
    
    /// 版本（用于乐观锁）
    pub version: u64,
    
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

/// Session 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// 空闲
    Idle,
    
    /// 登录中
    LoggingIn,
    
    /// 已激活（已登录）
    Active,
    
    /// 已过期
    Expired,
}

impl Session {
    pub fn new(device_id: String) -> Self {
        let now = Utc::now();
        Self {
            user_id: None,
            token: None,
            device_id,
            state: SessionState::Idle,
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// 开始登录
    pub fn start_login(&mut self) -> anyhow::Result<()> {
        if self.state != SessionState::Idle {
            return Err(anyhow::anyhow!("Session is not in Idle state"));
        }
        self.state = SessionState::LoggingIn;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 登录成功
    pub fn login_success(&mut self, user_id: String, token: String) -> anyhow::Result<()> {
        if self.state != SessionState::LoggingIn {
            return Err(anyhow::anyhow!("Session is not in LoggingIn state"));
        }
        self.user_id = Some(user_id);
        self.token = Some(token);
        self.state = SessionState::Active;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 登出
    pub fn logout(&mut self) -> anyhow::Result<()> {
        self.user_id = None;
        self.token = None;
        self.state = SessionState::Idle;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 标记过期
    pub fn expire(&mut self) -> anyhow::Result<()> {
        self.state = SessionState::Expired;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 检查是否已激活
    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_lifecycle() {
        let mut session = Session::new("device_123".to_string());
        
        // 初始状态应该是 Idle
        assert_eq!(session.state, SessionState::Idle);
        assert!(!session.is_active());
        
        // 开始登录
        session.start_login().unwrap();
        assert_eq!(session.state, SessionState::LoggingIn);
        
        // 登录成功
        session.login_success("user_123".to_string(), "token_123".to_string()).unwrap();
        assert_eq!(session.state, SessionState::Active);
        assert!(session.is_active());
        assert_eq!(session.user_id, Some("user_123".to_string()));
        assert_eq!(session.version, 1);
        
        // 登出
        session.logout().unwrap();
        assert_eq!(session.state, SessionState::Idle);
        assert!(!session.is_active());
        assert_eq!(session.user_id, None);
        assert_eq!(session.version, 2);
    }
    
    #[test]
    fn test_session_state_transitions() {
        let mut session = Session::new("device_123".to_string());
        
        // 不能从未登录状态直接登录成功
        assert!(session.login_success("user_123".to_string(), "token_123".to_string()).is_err());
        
        // 必须先开始登录
        session.start_login().unwrap();
        assert!(session.login_success("user_123".to_string(), "token_123".to_string()).is_ok());
        
        // 不能重复登录
        assert!(session.start_login().is_err());
    }
}
