//! Connection 聚合根
//!
//! 职责：管理长连接和心跳

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Connection 聚合根
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// 连接ID
    pub connection_id: Option<String>,
    
    /// 当前状态
    pub state: ConnectionState,
    
    /// 版本（用于乐观锁）
    pub version: u64,
    
    /// 最后心跳时间
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

/// Connection 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// 已断开
    Disconnected,
    
    /// 连接中
    Connecting,
    
    /// 在线
    Online,
    
    /// 重连中
    Reconnecting,
}

impl Connection {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            connection_id: None,
            state: ConnectionState::Disconnected,
            version: 0,
            last_heartbeat_at: None,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// 开始连接
    pub fn start_connect(&mut self) -> anyhow::Result<()> {
        if self.state != ConnectionState::Disconnected {
            return Err(anyhow::anyhow!("Connection is not in Disconnected state"));
        }
        self.state = ConnectionState::Connecting;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 连接成功
    pub fn connect_success(&mut self, connection_id: String) -> anyhow::Result<()> {
        if self.state != ConnectionState::Connecting {
            return Err(anyhow::anyhow!("Connection is not in Connecting state"));
        }
        self.connection_id = Some(connection_id);
        self.state = ConnectionState::Online;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 断开连接
    pub fn disconnect(&mut self) -> anyhow::Result<()> {
        self.connection_id = None;
        self.state = ConnectionState::Disconnected;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 开始重连
    pub fn start_reconnect(&mut self) -> anyhow::Result<()> {
        if self.state != ConnectionState::Online {
            return Err(anyhow::anyhow!("Connection is not in Online state"));
        }
        self.state = ConnectionState::Reconnecting;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 更新心跳
    pub fn update_heartbeat(&mut self) {
        self.last_heartbeat_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }
    
    /// 检查是否在线
    pub fn is_online(&self) -> bool {
        self.state == ConnectionState::Online
    }
}

impl Default for Connection {
    fn default() -> Self {
        Self::new()
    }
}
