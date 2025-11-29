//! 同步工具函数
//!
//! 提供消息保存、游标更新等公共逻辑

use crate::model::{Message, SyncCursor};
use crate::storage::StorageBackend;
use anyhow::Result;
use std::sync::Arc;

/// 消息保存策略
pub enum MessageSaveStrategy {
    /// 顺序保存（适用于少量消息）
    Sequential,
    /// 并行保存（适用于大量消息）
    Parallel { max_concurrency: usize },
}

impl Default for MessageSaveStrategy {
    fn default() -> Self {
        Self::Parallel { max_concurrency: 10 }
    }
}

/// 批量保存消息（优化版本）
/// 
/// 根据消息数量自动选择保存策略
/// 
/// # 参数
/// - `storage`: 存储后端
/// - `messages`: 要保存的消息列表
/// - `threshold`: 并行保存的阈值（消息数量超过此值使用并行保存）
/// 
/// # 返回
/// - `Result<()>`: 保存结果
pub async fn save_messages_optimized(
    storage: Arc<dyn StorageBackend>,
    messages: &[Message],
    threshold: usize,
) -> Result<()> {
    if messages.is_empty() {
        return Ok(());
    }
    
    if messages.len() > threshold {
        save_messages_parallel(storage, messages, 10).await
    } else {
        save_messages_sequential(storage, messages).await
    }
}

/// 顺序保存消息
async fn save_messages_sequential(
    storage: Arc<dyn StorageBackend>,
    messages: &[Message],
) -> Result<()> {
    for message in messages {
        storage.save_message(message).await?;
    }
    Ok(())
}

/// 并行保存消息（带并发限制）
async fn save_messages_parallel(
    storage: Arc<dyn StorageBackend>,
    messages: &[Message],
    max_concurrency: usize,
) -> Result<()> {
    use tokio::sync::Semaphore;
    #[cfg(target_arch = "wasm32")]
    use tokio::task::spawn_local as tokio_spawn;
    #[cfg(not(target_arch = "wasm32"))]
    use tokio::spawn as tokio_spawn;
    
    let semaphore = Arc::new(Semaphore::new(max_concurrency));
    let tasks: Vec<_> = messages.iter()
        .map(|msg| {
            let storage = Arc::clone(&storage);
            let sem = Arc::clone(&semaphore);
            let msg = msg.clone();
            tokio_spawn(async move {
                let _permit = sem.acquire().await
                    .map_err(|e| anyhow::anyhow!("Failed to acquire semaphore: {}", e))?;
                storage.save_message(&msg).await
            })
        })
        .collect();
    
    // 等待所有任务完成
    for task in tasks {
        task.await??;
    }
    Ok(())
}

/// 更新同步游标
/// 
/// # 参数
/// - `storage`: 存储后端
/// - `session_id`: 会话 ID
/// - `last_message`: 最后一条消息（可选）
/// - `extend_recent_range`: 是否扩展最近消息范围
pub async fn update_sync_cursor(
    storage: Arc<dyn StorageBackend>,
    session_id: &str,
    last_message: Option<&Message>,
    extend_recent_range: bool,
) -> Result<()> {
    if let Some(message) = last_message {
        if let Some(seq) = extract_seq_from_message(message) {
            let mut cursor = storage.get_sync_cursor(session_id).await?
                .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));
            
            // 扩展最近消息同步范围（如果需要）
            if extend_recent_range {
                if let Some((recent_start, recent_end)) = cursor.recent_sync_range {
                    if seq < recent_start {
                        cursor.update_recent_sync_range(seq, recent_end);
                    }
                }
            }
            
            cursor.last_seq = Some(seq);
            storage.save_sync_cursor(session_id, &cursor).await?;
        }
    }
    Ok(())
}

/// 从消息中提取 seq
fn extract_seq_from_message(message: &Message) -> Option<i64> {
    let seq_top = message.seq;
    if seq_top > 0 {
        Some(seq_top)
    } else {
        message
            .extra
            .get("seq")
            .and_then(|v| v.parse::<i64>().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::storage_trait::StorageBackend;
    use async_trait::async_trait;
    
    struct MockStorage;
    
    #[async_trait]
    impl StorageBackend for MockStorage {
        async fn save_message(&self, _message: &Message) -> Result<()> {
            Ok(())
        }
        
        // ... 其他方法使用默认实现或返回空
        async fn get_message(&self, _message_id: &str) -> Result<Option<Message>> {
            Ok(None)
        }
        
        async fn get_messages(
            &self,
            _session_id: &str,
            _limit: usize,
            _cursor: Option<String>,
        ) -> Result<Vec<Message>> {
            Ok(vec![])
        }
        
        async fn get_messages_by_seq(
            &self,
            _session_id: &str,
            _after_seq: i64,
            _limit: usize,
        ) -> Result<Vec<Message>> {
            Ok(vec![])
        }
        
        async fn get_max_seq(&self, _session_id: &str) -> Result<Option<i64>> {
            Ok(None)
        }
        
        async fn delete_message(&self, _message_id: &str) -> Result<()> {
            Ok(())
        }
        
        async fn save_session(&self, _session: &crate::model::SessionSummary) -> Result<()> {
            Ok(())
        }
        
        async fn get_session(&self, _session_id: &str) -> Result<Option<crate::model::SessionSummary>> {
            Ok(None)
        }
        
        async fn get_sessions(&self, _filter: crate::storage::SessionFilter) -> Result<Vec<crate::model::SessionSummary>> {
            Ok(vec![])
        }
        
        async fn update_session(
            &self,
            _session_id: &str,
            _updates: crate::storage::SessionUpdate,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn delete_session(&self, _session_id: &str) -> Result<()> {
            Ok(())
        }
        
        async fn save_sync_cursor(&self, _session_id: &str, _cursor: &SyncCursor) -> Result<()> {
            Ok(())
        }
        
        async fn get_sync_cursor(&self, _session_id: &str) -> Result<Option<SyncCursor>> {
            Ok(None)
        }
        
        async fn get_all_sync_cursors(&self) -> Result<Vec<SyncCursor>> {
            Ok(vec![])
        }
        
        async fn save_message_state(
            &self,
            _user_id: &str,
            _message_id: &str,
            _state: crate::storage::MessageState,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_message_state(
            &self,
            _user_id: &str,
            _message_id: &str,
        ) -> Result<Option<crate::storage::MessageState>> {
            Ok(None)
        }
        
        async fn batch_check_deleted(
            &self,
            _user_id: &str,
            _message_ids: &[String],
        ) -> Result<Vec<String>> {
            Ok(vec![])
        }
        
        async fn save_message_extension(
            &self,
            _message_id: &str,
            _extension: &crate::model::MessageExtension,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_message_extension(
            &self,
            _message_id: &str,
        ) -> Result<Option<crate::model::MessageExtension>> {
            Ok(None)
        }
        
        async fn save_session_extension(
            &self,
            _session_id: &str,
            _extension: &crate::model::SessionExtension,
        ) -> Result<()> {
            Ok(())
        }
        
        async fn get_session_extension(
            &self,
            _session_id: &str,
        ) -> Result<Option<crate::model::SessionExtension>> {
            Ok(None)
        }
        
        async fn batch_get_message_extensions(
            &self,
            _message_ids: &[String],
        ) -> Result<Vec<(String, crate::model::MessageExtension)>> {
            Ok(vec![])
        }
        
        async fn batch_get_session_extensions(
            &self,
            _session_ids: &[String],
        ) -> Result<Vec<(String, crate::model::SessionExtension)>> {
            Ok(vec![])
        }
    }
    
    impl crate::storage::storage_trait::StorageSyncBounds for MockStorage {}
    
    #[tokio::test]
    async fn test_save_messages_optimized() {
        let storage: Arc<dyn StorageBackend> = Arc::new(MockStorage);
        let messages = vec![Message::default(); 5];
        
        // 少量消息，应该使用顺序保存
        save_messages_optimized(Arc::clone(&storage), &messages, 10).await.unwrap();
        
        // 大量消息，应该使用并行保存
        let many_messages = vec![Message::default(); 20];
        save_messages_optimized(Arc::clone(&storage), &many_messages, 10).await.unwrap();
    }
}

