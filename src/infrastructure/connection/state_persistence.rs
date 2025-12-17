//! 状态持久化
//!
//! 参考 Telegram 设计，支持状态保存和恢复

use crate::infrastructure::connection::manager::ConnectionState;
use serde::{Deserialize, Serialize};

// 为 ConnectionState 实现序列化（用于状态持久化）
impl Serialize for ConnectionState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let state_str = match self {
            ConnectionState::Disconnected => "Disconnected",
            ConnectionState::Connecting => "Connecting",
            ConnectionState::Connected => "Connected",
            ConnectionState::Authenticating => "Authenticating",
            ConnectionState::Authenticated => "Authenticated",
            ConnectionState::Reconnecting => "Reconnecting",
        };
        serializer.serialize_str(state_str)
    }
}

impl<'de> Deserialize<'de> for ConnectionState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "Disconnected" => Ok(ConnectionState::Disconnected),
            "Connecting" => Ok(ConnectionState::Connecting),
            "Connected" => Ok(ConnectionState::Connected),
            "Authenticating" => Ok(ConnectionState::Authenticating),
            "Authenticated" => Ok(ConnectionState::Authenticated),
            "Reconnecting" => Ok(ConnectionState::Reconnecting),
            _ => Err(serde::de::Error::custom(format!(
                "Invalid connection state: {}",
                s
            ))),
        }
    }
}
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// 状态快照（用于持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// 连接状态
    pub state: ConnectionState,

    /// 状态时间戳（Unix 时间戳，秒）
    pub timestamp: u64,

    /// 状态元数据（可选）
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

impl StateSnapshot {
    /// 创建新的状态快照
    pub fn new(state: ConnectionState) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            state,
            timestamp,
            metadata: None,
        }
    }

    /// 检查快照是否过期
    pub fn is_expired(&self, max_age_secs: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        now.saturating_sub(self.timestamp) > max_age_secs
    }
}

/// 状态持久化接口
///
/// 实现此接口以支持状态持久化（如保存到文件、数据库等）
#[async_trait::async_trait]
pub trait StatePersistence: Send + Sync {
    /// 保存状态快照
    async fn save(&self, snapshot: &StateSnapshot) -> anyhow::Result<()>;

    /// 加载状态快照
    async fn load(&self) -> anyhow::Result<Option<StateSnapshot>>;

    /// 清除保存的状态
    async fn clear(&self) -> anyhow::Result<()>;
}

/// 内存状态持久化（用于测试或临时存储）
pub struct MemoryStatePersistence {
    snapshot: Arc<RwLock<Option<StateSnapshot>>>,
}

impl MemoryStatePersistence {
    pub fn new() -> Self {
        Self {
            snapshot: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait::async_trait]
impl StatePersistence for MemoryStatePersistence {
    async fn save(&self, snapshot: &StateSnapshot) -> anyhow::Result<()> {
        *self.snapshot.write().await = Some(snapshot.clone());
        Ok(())
    }

    async fn load(&self) -> anyhow::Result<Option<StateSnapshot>> {
        Ok(self.snapshot.read().await.clone())
    }

    async fn clear(&self) -> anyhow::Result<()> {
        *self.snapshot.write().await = None;
        Ok(())
    }
}

impl Default for MemoryStatePersistence {
    fn default() -> Self {
        Self::new()
    }
}

/// 状态历史记录（用于调试和审计）
pub struct StateHistory {
    /// 历史记录（最多保留 N 条）
    history: Arc<RwLock<Vec<StateSnapshot>>>,

    /// 最大历史记录数
    max_history: usize,
}

impl StateHistory {
    /// 创建新的状态历史记录器
    pub fn new(max_history: usize) -> Self {
        Self {
            history: Arc::new(RwLock::new(Vec::with_capacity(max_history))),
            max_history,
        }
    }

    /// 记录状态变化
    pub async fn record(&self, state: ConnectionState) {
        let snapshot = StateSnapshot::new(state);
        let mut history = self.history.write().await;

        history.push(snapshot);

        // 如果超过最大记录数，删除最旧的记录
        if history.len() > self.max_history {
            history.remove(0);
        }
    }

    /// 获取最近 N 条历史记录
    pub async fn recent(&self, count: usize) -> Vec<StateSnapshot> {
        let history = self.history.read().await;
        let start = history.len().saturating_sub(count);
        history[start..].to_vec()
    }

    /// 获取所有历史记录
    pub async fn all(&self) -> Vec<StateSnapshot> {
        self.history.read().await.clone()
    }

    /// 清除历史记录
    pub async fn clear(&self) {
        self.history.write().await.clear();
    }
}

impl Default for StateHistory {
    fn default() -> Self {
        Self::new(100) // 默认保留 100 条记录
    }
}
