//! 测试辅助工具模块
//!
//! 提供测试用的工具函数，包括：
//! - 服务端连接检查
//! - 消息验证
//! - 测试数据生成
//! - 等待和超时处理

use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// 检查服务端是否可用
///
/// 通过尝试连接 WebSocket 来检查服务端是否运行
pub async fn check_server_available(server_url: &str) -> bool {
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    
    debug!("检查服务端是否可用: {}", server_url);
    
    match tokio::time::timeout(Duration::from_secs(2), connect_async(server_url)).await {
        Ok(Ok((mut stream, _))) => {
            // 尝试关闭连接
            let _ = stream.close(None).await;
            info!("✅ 服务端可用: {}", server_url);
            true
        }
        Ok(Err(e)) => {
            warn!("服务端不可用: {} - {}", server_url, e);
            false
        }
        Err(_) => {
            warn!("服务端连接超时: {}", server_url);
            false
        }
    }
}

/// 等待条件满足（带超时）
///
/// 每隔 `interval` 检查一次 `condition`，直到返回 `true` 或超时
pub async fn wait_for_condition<F>(condition: F, timeout: Duration, interval: Duration) -> bool
where
    F: Fn() -> bool,
{
    let start = Instant::now();
    
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        sleep(interval).await;
    }
    
    false
}

/// 等待异步条件满足（带超时）
///
/// 每隔 `interval` 检查一次 `condition`，直到返回 `true` 或超时
pub async fn wait_for_async_condition<F, Fut>(condition: F, timeout: Duration, interval: Duration) -> bool
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = Instant::now();
    
    while start.elapsed() < timeout {
        if condition().await {
            return true;
        }
        sleep(interval).await;
    }
    
    false
}

/// 等待消息到达（带超时）
///
/// 从消息队列中接收指定会话的消息
pub async fn wait_for_message(
    queue: std::sync::Arc<flare_im_core_sdk::domain::message_queue::MessageQueue>,
    conversation_id: &str,
    timeout: Duration,
) -> Option<flare_im_core_sdk::domain::message::Message> {
    let start = Instant::now();
    let interval = Duration::from_millis(100);
    
    while start.elapsed() < timeout {
        // 尝试从队列中接收消息
        if let Some(queued_msg) = queue.try_receive().await {
            if queued_msg.message.conversation_id == conversation_id {
                debug!(
                    "找到目标会话的消息: conversation_id={}, message_id={}",
                    conversation_id, queued_msg.message.id
                );
                return Some(queued_msg.message);
            } else {
                // 不是目标会话的消息，重新入队（保持原优先级）
                let _ = queue.enqueue(queued_msg.message, queued_msg.priority).await;
            }
        }
        
        // 如果队列为空，等待一段时间再检查
        if queue.is_empty().await {
            sleep(interval).await;
        } else {
            // 队列有消息，快速检查
            sleep(Duration::from_millis(10)).await;
        }
    }
    
    warn!(
        "等待消息超时: conversation_id={}, timeout={:?}",
        conversation_id, timeout
    );
    None
}

/// 生成测试用户 ID
pub fn generate_test_user_id(prefix: &str, index: u32) -> String {
    format!("{}_test_{:04}", prefix, index)
}

/// 生成测试会话 ID
///
/// 使用 flare-core 的标准会话 ID 生成函数
pub fn generate_test_conversation_id(user1: &str, user2: &str) -> String {
    flare_core::generate_single_chat_conversation_id(user1, user2)
}

/// 等待连接建立
pub async fn wait_for_connection(
    sdk: &flare_im_core_sdk::interface::facade::ImCoreSdk,
    timeout: Duration,
) -> bool {
    wait_for_async_condition(
        || async {
            // 检查连接状态（需要根据实际 API 调整）
            // 暂时使用简单的等待策略
            true
        },
        timeout,
        Duration::from_millis(100),
    )
    .await
}

/// 验证消息内容
pub fn verify_message(
    message: &flare_im_core_sdk::domain::message::Message,
    expected_conversation_id: &str,
    expected_sender_id: &str,
    expected_content: Option<&str>,
) -> bool {
    let mut valid = true;
    
    if message.conversation_id != expected_conversation_id {
        warn!(
            "消息会话 ID 不匹配: 期望 {}, 实际 {}",
            expected_conversation_id, message.conversation_id
        );
        valid = false;
    }
    
    if message.sender_id != expected_sender_id {
        warn!(
            "消息发送者 ID 不匹配: 期望 {}, 实际 {}",
            expected_sender_id, message.sender_id
        );
        valid = false;
    }
    
    if let Some(expected) = expected_content {
        if !message.content.is_empty() {
            // content 是 Vec<u8>，需要转换为字符串比较
            let content_str = String::from_utf8_lossy(&message.content);
            if content_str != expected {
                warn!(
                    "消息内容不匹配: 期望 {}, 实际 {}",
                    expected, content_str
                );
                valid = false;
            }
        } else {
            warn!("消息内容为空，但期望有内容");
            valid = false;
        }
    }
    
    valid
}

/// 测试配置
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// 服务端地址
    pub server_url: String,
    /// QUIC 地址（可选）
    pub quic_url: Option<String>,
    /// 连接超时
    pub connect_timeout: Duration,
    /// 消息等待超时
    pub message_timeout: Duration,
    /// 是否保留测试数据库
    pub keep_test_db: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            server_url: std::env::var("FLARE_TEST_SERVER_URL")
                .unwrap_or_else(|_| "ws://localhost:60051".to_string()),
            quic_url: std::env::var("FLARE_TEST_QUIC_URL").ok(),
            connect_timeout: Duration::from_secs(10),
            message_timeout: Duration::from_secs(30),
            keep_test_db: std::env::var("FLARE_KEEP_TEST_DB")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
        }
    }
}

/// 创建测试用的 SDK 实例（使用 TestConfig）
pub async fn create_test_sdk_with_config(
    config: &TestConfig,
) -> (
    flare_im_core_sdk::interface::facade::ImCoreSdk,
    tempfile::TempDir,
    std::path::PathBuf,
) {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_path = temp_dir.path().join("storage");
    let db_path = storage_path.join("flare_im.db");
    
    // 确保存储目录存在
    std::fs::create_dir_all(&storage_path).unwrap();
    
    let mut config_builder = flare_im_core_sdk::config::SdkConfig::builder()
        .websocket_url(&config.server_url)
        .storage_path(&storage_path)
        .media_cache_path(temp_dir.path().join("media_cache"))
        .log_level("error"); // 测试时减少日志输出
    
    // 如果配置了 QUIC URL，添加 QUIC 支持
    if let Some(ref quic) = config.quic_url {
        config_builder = config_builder
            .quic_url(quic)
            .quic_disable_cert_verify(); // 测试环境禁用证书验证
    }
    
    let sdk_config = config_builder.build();
    
    let sdk = flare_im_core_sdk::interface::facade::ImCoreSdk::new(sdk_config)
        .await
        .unwrap();
    
    // 如果设置了保留数据库文件，输出路径信息
    if config.keep_test_db {
        println!("📁 测试数据库文件位置: {}", db_path.display());
        println!("📁 临时目录: {}", temp_dir.path().display());
    }
    
    (sdk, temp_dir, db_path)
}

