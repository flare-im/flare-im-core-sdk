//! 共享工具函数
//!
//! 提供跨模块使用的通用工具函数

/// 生成消息 ID
///
/// 根据平台选择不同的生成策略：
/// - WASM: 使用时间戳 + 计数器
/// - 其他平台: 使用 UUID
#[cfg(target_arch = "wasm32")]
pub fn new_message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = chrono::Utc::now().timestamp_millis();
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("msg-{}-{}", ts, c)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn new_message_id() -> String {
    format!("msg-{}", uuid::Uuid::new_v4())
}

/// 生成唯一的消息 ID（带冲突检测）
///
/// # 参数
/// - `storage`: 存储后端，用于检查冲突
/// - `max_attempts`: 最大尝试次数，默认10次
///
/// # 返回
/// - `Ok(String)`: 唯一的消息 ID
/// - `Err(_)`: 无法生成唯一 ID
pub async fn generate_unique_message_id(
    storage: &dyn crate::infrastructure::storage::StorageBackend,
    max_attempts: Option<u32>,
) -> anyhow::Result<String> {
    const DEFAULT_MAX_ATTEMPTS: u32 = 10;
    let max_attempts = max_attempts.unwrap_or(DEFAULT_MAX_ATTEMPTS);

    for _ in 0..max_attempts {
        let message_id = new_message_id();

        // 检查本地是否已存在该消息ID
        match storage.get_message(&message_id).await {
            Ok(Some(_)) => {
                // ID已存在，继续尝试
                continue;
            }
            Ok(None) => {
                // ID不存在，可以使用
                return Ok(message_id);
            }
            Err(e) => {
                // 存储错误，返回错误
                return Err(anyhow::anyhow!(
                    "Failed to check message ID uniqueness: {}",
                    e
                ));
            }
        }
    }

    Err(anyhow::anyhow!(
        "Failed to generate unique message ID after {} attempts",
        max_attempts
    ))
}
