//! 同步任务检查点
//!
//! 任务可选择性持久化游标（cursor），以便中断后恢复。引擎在运行任务前加载 checkpoint，
//! 任务完成后可保存新 cursor。使用 [crate::domain::SyncCursorStore]，key 格式：`sync:checkpoint:{task_id}`.

use crate::domain::SyncCursorStore;
use crate::shared::error::Result;

const PREFIX: &str = "sync:checkpoint:";

fn key(task_id: &str) -> String {
    format!("{PREFIX}{task_id}")
}

/// 单个任务的检查点（游标）
#[derive(Clone, Debug, Default)]
pub struct SyncCheckpoint {
    pub task_id: String,
    pub cursor: Option<String>,
}

impl SyncCheckpoint {
    pub fn new(task_id: String, cursor: Option<String>) -> Self {
        Self { task_id, cursor }
    }
}

/// 检查点存储抽象（基于 SyncCursorStore）
pub struct CheckpointStore {
    cursors: std::sync::Arc<dyn SyncCursorStore>,
}

impl CheckpointStore {
    pub fn new(cursors: std::sync::Arc<dyn SyncCursorStore>) -> Self {
        Self { cursors }
    }

    /// 加载任务检查点
    pub async fn load(&self, task_id: &str) -> Result<SyncCheckpoint> {
        let k = key(task_id);
        let cursor = self.cursors.get_raw(&k).await?;
        Ok(SyncCheckpoint::new(task_id.to_string(), cursor))
    }

    /// 保存任务检查点
    pub async fn save(&self, cp: &SyncCheckpoint) -> Result<()> {
        let k = key(&cp.task_id);
        match &cp.cursor {
            Some(c) => self.cursors.save_raw(&k, c).await,
            None => Ok(()), // 不写入空游标，保留旧值或由调用方删除
        }
    }
}
