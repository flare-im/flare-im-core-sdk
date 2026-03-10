//! 测试公共基础设施
//!
//! - MemoryMessageStore / MemoryConversationStore / MemoryCursorStore：纯内存实现
//! - create_test_client / create_test_client_no_connect：快速创建 IMClient
//! - establish_connection：连接到服务端并等待 Ready
//! - SERIAL_LOCK：串行化集成测试，避免同平台互斥踢下线

#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use async_trait::async_trait;
use once_cell::sync::Lazy;
use tokio::sync::RwLock;

use flare_im_core_sdk::prelude::*;
use flare_im_core_sdk::model::message::Message;
use flare_im_core_sdk::model::conversation::ConversationSummary;

/// 串行化集成测试，同一用户仅允许一个连接在线
pub static SERIAL_LOCK: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

// =============================================================================
// MemoryMessageStore
// =============================================================================

pub struct MemoryMessageStore {
    data: RwLock<HashMap<String, Message>>,
}

impl MemoryMessageStore {
    pub fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }
}

#[async_trait]
impl MessageStore for MemoryMessageStore {
    async fn save_batch(&self, messages: &[Message]) -> Result<()> {
        let mut data = self.data.write().await;
        for msg in messages {
            let key = if !msg.server_id.is_empty() {
                msg.server_id.clone()
            } else {
                msg.client_msg_id.clone()
            };
            data.insert(key, msg.clone());
        }
        Ok(())
    }

    async fn get(&self, message_id: &str) -> Result<Option<Message>> {
        let data = self.data.read().await;
        Ok(data.get(message_id).cloned())
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<Message>> {
        let data = self.data.read().await;
        let mut msgs: Vec<_> = data.values()
            .filter(|m| m.conversation_id == conversation_id && m.seq < before_seq)
            .cloned()
            .collect();
        msgs.sort_by(|a, b| b.seq.cmp(&a.seq));
        msgs.truncate(limit as usize);
        Ok(msgs)
    }

    async fn update_status(&self, message_id: &str, status: i32) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(msg) = data.get_mut(message_id) {
            msg.status = status;
        }
        Ok(())
    }

    async fn update_content(&self, message_id: &str, new_content: Vec<u8>) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(msg) = data.get_mut(message_id) {
            msg.content = new_content;
        }
        Ok(())
    }

    async fn delete(&self, message_id: &str) -> Result<()> {
        let mut data = self.data.write().await;
        data.remove(message_id);
        Ok(())
    }

    async fn search(&self, _keyword: &str, limit: u32) -> Result<Vec<Message>> {
        let data = self.data.read().await;
        let results: Vec<_> = data.values()
            .cloned()
            .take(limit as usize)
            .collect();
        Ok(results)
    }
}

// =============================================================================
// MemoryConversationStore
// =============================================================================

pub struct MemoryConversationStore {
    data: RwLock<HashMap<String, ConversationSummary>>,
}

impl MemoryConversationStore {
    pub fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }
}

#[async_trait]
impl ConversationStore for MemoryConversationStore {
    async fn save_batch(&self, conversations: &[ConversationSummary]) -> Result<()> {
        let mut data = self.data.write().await;
        for conv in conversations {
            data.insert(conv.conversation_id.clone(), conv.clone());
        }
        Ok(())
    }

    async fn get(&self, conversation_id: &str) -> Result<Option<ConversationSummary>> {
        let data = self.data.read().await;
        Ok(data.get(conversation_id).cloned())
    }

    async fn list(&self) -> Result<Vec<ConversationSummary>> {
        let data = self.data.read().await;
        Ok(data.values().cloned().collect())
    }

    async fn update_unread(
        &self,
        conversation_id: &str,
        unread_count: u32,
        _last_read_seq: u64,
    ) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(conv) = data.get_mut(conversation_id) {
            conv.unread_count = unread_count;
        }
        Ok(())
    }

    async fn delete(&self, conversation_id: &str) -> Result<()> {
        let mut data = self.data.write().await;
        data.remove(conversation_id);
        Ok(())
    }
}

// =============================================================================
// MemoryCursorStore
// =============================================================================

pub struct MemoryCursorStore {
    data: RwLock<HashMap<String, String>>,
}

impl MemoryCursorStore {
    pub fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }
}

#[async_trait]
impl SyncCursorStore for MemoryCursorStore {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn save(&self, key: &str, cursor: &str) -> Result<()> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), cursor.to_string());
        Ok(())
    }
}

// =============================================================================
// Helper functions
// =============================================================================

fn get_ws_url() -> String {
    env::var("FLARE_TEST_SERVER_URL")
        .unwrap_or_else(|_| "ws://localhost:60051".to_string())
}

fn make_stores() -> StoreProvider {
    StoreProvider {
        messages: Arc::new(MemoryMessageStore::new()),
        conversations: Arc::new(MemoryConversationStore::new()),
        cursors: Arc::new(MemoryCursorStore::new()),
    }
}

/// 创建 IMClient（不连接），用于纯本地测试
pub async fn create_test_client_no_connect() -> IMClient {
    let config = SdkConfig::new(get_ws_url());
    IMClient::builder()
        .config(config)
        .stores(make_stores())
        .build()
}

/// 创建 IMClient（用于集成测试，需服务端运行）
pub async fn create_test_client() -> IMClient {
    create_test_client_no_connect().await
}

/// 连接到服务端并等待 SDK 进入 Ready 状态
///
/// 包含 connect → bootstrap → 等待 Ready 全流程。
/// 内置重试机制：服务端可能因前次测试的设备冲突清理未完成而暂时拒绝连接，
/// 最多重试 3 次（间隔 2s）以等待服务端清理旧连接状态。
pub async fn establish_connection(client: &mut IMClient, user_id: &str) {
    let token = generate_test_token(user_id);

    let max_retries = 3;
    let mut last_err = String::new();
    for attempt in 1..=max_retries {
        match client.connect(user_id, &token).await {
            Ok(()) => {
                last_err.clear();
                break;
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt < max_retries {
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                }
            }
        }
    }
    if !last_err.is_empty() {
        panic!("连接失败（user={user_id}，已重试 {max_retries} 次）: {last_err}。请确保服务端已启动");
    }

    let max_wait = tokio::time::Duration::from_secs(15);
    let start = tokio::time::Instant::now();
    loop {
        if client.state() == SdkState::Ready {
            break;
        }
        if start.elapsed() > max_wait {
            panic!(
                "SDK 在 {max_wait:?} 内未进入 Ready 状态（当前: {:?}）",
                client.state()
            );
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
}

/// 断开连接 + 等待服务端清理
///
/// 集成测试结束时调用，确保服务端有时间完成设备冲突清理。
pub async fn teardown(client: &mut IMClient) {
    let _ = client.disconnect().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
}

/// 构建可直接发送的单聊文本消息
///
/// 设置 receiver_id 和 conversation_type = Single，满足服务端对单聊消息的要求。
pub fn build_single_text(
    conversation_id: &str,
    sender_id: &str,
    receiver_id: &str,
    text: &str,
) -> flare_im_core_sdk::model::message::Message {
    use flare_im_core_sdk::model::{ContentBuilder, MessageBuilder};
    MessageBuilder::new(conversation_id, sender_id)
        .content(ContentBuilder::text(text).build())
        .receiver(receiver_id)
        .single_chat()
        .build()
        .expect("build single text message")
}

/// 生成测试 JWT token（使用 SDK 内置的 generate_test_token）
///
/// 与 flare-im-core/examples/chatroom_client.rs 保持一致：
/// secret = "insecure-secret"，issuer = "flare-im-core"，tenant_id = "default"
fn generate_test_token(user_id: &str) -> String {
    flare_im_core_sdk::util::generate_test_token(
        "insecure-secret",
        "flare-im-core",
        user_id,
        3600,
        None,
        Some("default"),
    ).expect("generate_test_token should not fail")
}
