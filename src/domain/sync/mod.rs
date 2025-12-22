//! Sync 聚合根
//!
//! 职责：管理同步状态

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Sync 聚合根
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sync {
    /// 当前状态
    pub state: SyncState,
    
    /// Bootstrap Sync 游标
    pub bootstrap_cursor: Option<String>,
    
    /// Async Sync 游标（按类型存储）
    pub async_cursors: std::collections::HashMap<String, String>,
    
    /// 版本（用于乐观锁）
    pub version: u64,
    
    /// 创建时间
    pub created_at: DateTime<Utc>,
    
    /// 更新时间
    pub updated_at: DateTime<Utc>,
}

/// Sync 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncState {
    /// 空闲
    Idle,
    
    /// Bootstrap 同步中
    Bootstrapping,
    
    /// 就绪（Bootstrap 完成）
    Ready,
    
    /// 异步同步中
    Syncing,
}

impl Sync {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            state: SyncState::Idle,
            bootstrap_cursor: None,
            async_cursors: std::collections::HashMap::new(),
            version: 0,
            created_at: now,
            updated_at: now,
        }
    }
    
    /// 开始 Bootstrap Sync
    pub fn start_bootstrap(&mut self) -> anyhow::Result<()> {
        if self.state != SyncState::Idle {
            return Err(anyhow::anyhow!("Sync is not in Idle state"));
        }
        self.state = SyncState::Bootstrapping;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// Bootstrap Sync 完成
    pub fn bootstrap_completed(&mut self, cursor: String) -> anyhow::Result<()> {
        if self.state != SyncState::Bootstrapping {
            return Err(anyhow::anyhow!("Sync is not in Bootstrapping state"));
        }
        self.bootstrap_cursor = Some(cursor);
        self.state = SyncState::Ready;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// Bootstrap Sync 失败
    pub fn bootstrap_failed(&mut self) -> anyhow::Result<()> {
        if self.state != SyncState::Bootstrapping {
            return Err(anyhow::anyhow!("Sync is not in Bootstrapping state"));
        }
        self.state = SyncState::Idle;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 开始 Async Sync
    pub fn start_async(&mut self, sync_type: String) -> anyhow::Result<()> {
        if self.state != SyncState::Ready {
            return Err(anyhow::anyhow!("Sync is not in Ready state"));
        }
        self.state = SyncState::Syncing;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// Async Sync 完成
    pub fn async_completed(&mut self, sync_type: String, cursor: String) -> anyhow::Result<()> {
        if self.state != SyncState::Syncing {
            return Err(anyhow::anyhow!("Sync is not in Syncing state"));
        }
        self.async_cursors.insert(sync_type, cursor);
        self.state = SyncState::Ready;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// Async Sync 失败
    pub fn async_failed(&mut self) -> anyhow::Result<()> {
        if self.state != SyncState::Syncing {
            return Err(anyhow::anyhow!("Sync is not in Syncing state"));
        }
        self.state = SyncState::Ready;
        self.version += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
    
    /// 检查是否就绪
    pub fn is_ready(&self) -> bool {
        self.state == SyncState::Ready
    }
}

impl Default for Sync {
    fn default() -> Self {
        Self::new()
    }
}
