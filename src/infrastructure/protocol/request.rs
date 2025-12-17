//! 请求/响应管理器
//!
//! 用于管理请求/响应模式，通过 request_id 匹配请求和响应

use flare_core::common::protocol::Frame;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
#[cfg(not(target_arch = "wasm32"))]
use uuid::Uuid;

/// 待处理的请求（带时间戳）
struct PendingRequest {
    sender: oneshot::Sender<Frame>,
    created_at: Instant,
}

/// 请求管理器（用于管理请求/响应）
/// 
/// 优化：支持超时清理，防止内存泄漏
pub struct RequestManager {
    pending_requests: Arc<Mutex<HashMap<String, PendingRequest>>>,
    /// 超时清理任务句柄
    cleanup_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// 默认超时时间（秒）
    default_timeout: u64,
}

impl RequestManager {
    /// 创建新的请求管理器
    /// 
    /// # 参数
    /// - `default_timeout`: 默认超时时间（秒），默认 60 秒
    pub fn new() -> Self {
        Self::with_timeout(60)
    }
    
    /// 创建指定超时时间的请求管理器
    pub fn with_timeout(default_timeout: u64) -> Self {
        let pending_requests: Arc<Mutex<HashMap<String, PendingRequest>>> = Arc::new(Mutex::new(HashMap::new()));
        let cleanup_handle = Arc::new(Mutex::new(None));
        let timeout = default_timeout;
        
        // 启动自动清理任务
        let pending_clone = Arc::clone(&pending_requests);
        let handle_clone = Arc::clone(&cleanup_handle);
        
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                
                let now = Instant::now();
                let mut requests = pending_clone.lock().await;
                let before_count = requests.len();
                
                // 清理超时请求
                requests.retain(|_, req| {
                    now.duration_since(req.created_at).as_secs() < timeout
                });
                
                let after_count = requests.len();
                if before_count != after_count {
                    tracing::debug!(
                        cleaned = before_count - after_count,
                        remaining = after_count,
                        "Cleaned up timeout requests"
                    );
                }
            }
        });
        
        // 异步设置清理句柄
        tokio::spawn(async move {
            *handle_clone.lock().await = Some(handle);
        });
        
        Self {
            pending_requests,
            cleanup_handle,
            default_timeout,
        }
    }
    
    /// 创建请求并返回响应接收器
    /// 
    /// # 返回
    /// - `request_id`: 请求 ID（用于匹配响应）
    /// - `receiver`: 响应接收器
    /// 创建新的请求并注册响应通道，返回 `request_id` 与接收器
    #[tracing::instrument(name = "request.create", skip(self))]
    pub async fn create_request(&self) -> (String, oneshot::Receiver<Frame>) {
        let request_id = new_request_id();
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(request_id.clone(), PendingRequest {
                sender: tx,
                created_at: Instant::now(),
            });
            tracing::debug!(%request_id, pending_count = pending.len(), "Registered pending request");
        }
        (request_id, rx)
    }
    
    /// 完成请求（收到响应）
    /// 
    /// # 参数
    /// - `request_id`: 请求 ID
    /// - `response`: 响应 Frame
    /// 
    /// # 返回
    /// - `Ok(())`: 成功
    /// - `Err`: 请求 ID 不存在或发送失败
    /// 收到响应后完成并唤醒等待方，若未找到则记录错误便于排查
    #[tracing::instrument(name = "request.complete", skip(self, response), fields(msg_id = %response.message_id))]
    pub async fn complete_request(&self, request_id: &str, response: Frame) -> anyhow::Result<()> {
        let mut pending = self.pending_requests.lock().await;
        if let Some(req) = pending.remove(request_id) {
            tracing::debug!(%request_id, remaining = pending.len(), "Completing request");
            req.sender.send(response)
                .map_err(|_| anyhow::anyhow!("Failed to send response: receiver dropped"))?;
            Ok(())
        } else {
            tracing::warn!(%request_id, remaining = pending.len(), "Request ID not found when completing");
            Err(anyhow::anyhow!("Request ID not found: {}", request_id))
        }
    }
    
    /// 取消请求（超时或取消）
    pub async fn cancel_request(&self, request_id: &str) {
        self.pending_requests.lock().await.remove(request_id);
    }
    
    /// 获取待处理的请求数量
    pub async fn pending_count(&self) -> usize {
        self.pending_requests.lock().await.len()
    }
    
    /// 手动清理超时的请求
    /// 
    /// 通常不需要手动调用，自动清理任务会定期执行
    /// 
    /// # 参数
    /// - `timeout`: 超时时间（秒），如果为 None 则使用默认超时时间
    pub async fn cleanup_timeout_requests(&self, timeout: Option<u64>) {
        let timeout_secs = timeout.unwrap_or(self.default_timeout);
        let now = Instant::now();
        let mut pending = self.pending_requests.lock().await;
        
        let before_count = pending.len();
        pending.retain(|_, req| {
            now.duration_since(req.created_at).as_secs() < timeout_secs
        });
        
        let after_count = pending.len();
        if before_count != after_count {
            tracing::info!(
                cleaned = before_count - after_count,
                remaining = after_count,
                timeout_secs = timeout_secs,
                "Manually cleaned up timeout requests"
            );
        }
    }
    
    /// 停止自动清理任务
    pub async fn stop_cleanup_task(&self) {
        if let Some(handle) = self.cleanup_handle.lock().await.take() {
            handle.abort();
            tracing::debug!("RequestManager cleanup task stopped");
        }
    }
    
    /// 清理所有待处理的请求
    pub async fn clear_all(&self) {
        let mut pending = self.pending_requests.lock().await;
        let count = pending.len();
        pending.clear();
        if count > 0 {
            tracing::info!(cleared_count = count, "Cleared all pending requests");
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn new_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // 优化：使用 chrono 避免 unwrap
    let ts = chrono::Utc::now().timestamp_millis();
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("req-{}-{}", ts, c)
}

#[cfg(not(target_arch = "wasm32"))]
fn new_request_id() -> String { Uuid::new_v4().to_string() }

impl Default for RequestManager {
    fn default() -> Self {
        Self::with_timeout(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_core::common::protocol::{Frame, Reliability, Command};
    use flare_core::common::protocol::builder::ping;
    use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
    
    #[tokio::test]
    async fn test_request_manager() {
        let manager = RequestManager::new();
        
        // 创建请求
        let (request_id, mut receiver) = manager.create_request().await;
        
        // 模拟响应
        let response = Frame {
            command: Some(Command {
                r#type: Some(CommandType::System(ping())),
            }),
            message_id: "response-123".to_string(),
            reliability: Reliability::AtLeastOnce as i32,
            timestamp: 1234567890,
            metadata: Default::default(),
        };
        
        // 完成请求
        manager.complete_request(&request_id, response.clone()).await.unwrap();
        
        // 接收响应
        let received = receiver.await.unwrap();
        assert_eq!(received.message_id, response.message_id);
    }
    
    #[tokio::test]
    async fn test_request_timeout() {
        let manager = RequestManager::new();
        
        // 创建请求
        let (request_id, _receiver) = manager.create_request().await;
        
        // 取消请求
        manager.cancel_request(&request_id).await;
        
        // 尝试完成已取消的请求应该失败
        let response = Frame {
            command: Some(Command {
                r#type: Some(CommandType::System(ping())),
            }),
            message_id: "response-123".to_string(),
            reliability: Reliability::AtLeastOnce as i32,
            timestamp: 1234567890,
            metadata: Default::default(),
        };
        
        let result = manager.complete_request(&request_id, response).await;
        assert!(result.is_err());
    }
}
