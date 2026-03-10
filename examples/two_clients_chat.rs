//! 一对一聊天客户端示例（基于当前 SDK API）
//!
//! 展示如何使用 IMClient、MessageApi、ConversationApi 与事件回调，
//! 所有可能失败的操作均做错误处理，无 unwrap/expect。
//!
//! ## 运行方式
//!
//! ```bash
//! RUST_LOG=info cargo run --example two_clients_chat
//! MY_USER_ID=user-alice CHAT_WITH=user-bob SERVER_URL=ws://localhost:60051 cargo run --example two_clients_chat
//! MY_USER_ID=user-bob CHAT_WITH=user-alice SERVER_URL=ws://localhost:60051 cargo run --example two_clients_chat
//! ```

use anyhow::Context;
use flare_im_core_sdk::model::conversation::ConversationSummary;
use flare_im_core_sdk::prelude::*;
use flare_im_core_sdk::util::generate_test_token;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ── 内存存储（示例用，与 tests/common 逻辑一致）────────────────────────

struct MemoryMessageStore {
    data: RwLock<HashMap<String, Message>>,
}

impl MemoryMessageStore {
    fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl MessageStore for MemoryMessageStore {
    async fn save_batch(&self, messages: &[Message]) -> Result<()> {
        let mut data = self.data.write().await;
        for msg in messages {
            let key = if !msg.server_id.is_empty() { msg.server_id.clone() } else { msg.client_msg_id.clone() };
            data.insert(key, msg.clone());
        }
        Ok(())
    }
    async fn get(&self, message_id: &str) -> Result<Option<Message>> {
        let data = self.data.read().await;
        Ok(data.get(message_id).cloned())
    }
    async fn get_by_conversation(&self, conversation_id: &str, before_seq: u64, limit: u32) -> Result<Vec<Message>> {
        let data = self.data.read().await;
        let mut msgs: Vec<_> = data
            .values()
            .filter(|m| m.conversation_id == conversation_id && (m.seq as u64) < before_seq)
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
        Ok(data.values().cloned().take(limit as usize).collect())
    }
}

struct MemoryConversationStore {
    data: RwLock<HashMap<String, ConversationSummary>>,
}

impl MemoryConversationStore {
    fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl ConversationStore for MemoryConversationStore {
    async fn save_batch(&self, conversations: &[ConversationSummary]) -> Result<()> {
        let mut data = self.data.write().await;
        for c in conversations {
            data.insert(c.conversation_id.clone(), c.clone());
        }
        Ok(())
    }
    async fn get(&self, conversation_id: &str) -> Result<Option<ConversationSummary>> {
        Ok(self.data.read().await.get(conversation_id).cloned())
    }
    async fn list(&self) -> Result<Vec<ConversationSummary>> {
        Ok(self.data.read().await.values().cloned().collect())
    }
    async fn update_unread(&self, conversation_id: &str, unread_count: u32, _last_read_seq: u64) -> Result<()> {
        let mut data = self.data.write().await;
        if let Some(c) = data.get_mut(conversation_id) {
            c.unread_count = unread_count;
        }
        Ok(())
    }
    async fn delete(&self, conversation_id: &str) -> Result<()> {
        self.data.write().await.remove(conversation_id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl SyncCursorStore for MemoryCursorStore {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.data.read().await.get(key).cloned())
    }
    async fn save(&self, key: &str, cursor: &str) -> Result<()> {
        self.data.write().await.insert(key.to_string(), cursor.to_string());
        Ok(())
    }
}

struct MemoryCursorStore {
    data: RwLock<HashMap<String, String>>,
}

impl MemoryCursorStore {
    fn new() -> Self {
        Self { data: RwLock::new(HashMap::new()) }
    }
}

fn make_stores() -> StoreProvider {
    StoreProvider {
        messages: Arc::new(MemoryMessageStore::new()),
        conversations: Arc::new(MemoryConversationStore::new()),
        cursors: Arc::new(MemoryCursorStore::new()),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .try_init()
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;

    info!("🚀 Flare IM SDK 一对一聊天示例");
    info!("==========================================");

    let ws_url = std::env::var("SERVER_URL").unwrap_or_else(|_| "ws://localhost:60051".to_string());
    let my_user_id = std::env::var("MY_USER_ID").unwrap_or_else(|_| format!("user-{}", std::process::id()));
    let chat_with_env = std::env::var("CHAT_WITH").unwrap_or_else(|_| String::new());

    info!("服务器: {}", ws_url);
    info!("当前用户: {}", my_user_id);

    let chat_with = if chat_with_env.is_empty() {
        info!("请输入聊天对象 user_id（按 Enter 确认）:");
        let mut input = String::new();
        let mut reader = io::BufReader::new(io::stdin());
        reader.read_line(&mut input).await.context("读取 stdin 失败")?;
        let s = input.trim().to_string();
        if s.is_empty() {
            anyhow::bail!("聊天对象不能为空");
        }
        s
    } else {
        chat_with_env
    };
    info!("聊天对象: {}", chat_with);

    let token = generate_test_token(
        "insecure-secret",
        "flare-im-core",
        &my_user_id,
        3600,
        None,
        Some("default"),
    ).context("生成 Token 失败")?;

    let config = SdkConfig::new(&ws_url);
    let stores = make_stores();
    let mut client = IMClient::builder().config(config).stores(stores).build();

    // 必须保留 Subscription，否则回调任务可能被取消，收不到对方消息
    let _sub_msg = client.on_message(|msg| {
        if let Ok(decoded) = decode_content(msg) {
            info!("[收到] {}", decoded.text_preview());
        }
    });
    let _sub_ev = client.on(|e| {
        if let SdkEvent::ServerError { code, message } = e.as_ref() {
            warn!("服务端错误 code={} message={}", code, message);
        }
    });

    info!("正在连接...");
    client.connect(&my_user_id, &token).await.context("连接失败")?;

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
    while client.state() != SdkState::Ready {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("等待 Ready 超时");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
    info!("已就绪");

    if let Err(e) = client.sync_conversations().await {
        warn!("同步会话列表失败（可继续）: {}", e);
    }

    let conversation_id = client.conversation().single_chat_id(&my_user_id, &chat_with);
    info!("会话ID: {}", conversation_id);

    if let Ok(convs) = client.conversation().list().await {
        info!("会话数: {}", convs.len());
    }

    info!("");
    info!("输入消息按 Enter 发送；/list 会话列表；/history 历史；/read 标记已读；/quit 退出");
    info!("");

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
    let client_tx = Arc::new(RwLock::new(client));

    let input_handle = {
        let tx = tx.clone();
        let conv_id = conversation_id.clone();
        let _my_id = my_user_id.clone();
        let _recv_id = chat_with.clone();
        let client_ref = Arc::clone(&client_tx);
        tokio::spawn(async move {
            let stdin = io::stdin();
            let mut reader = io::BufReader::new(stdin);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let input = line.trim().to_string();
                        if input.is_empty() {
                            continue;
                        }
                        if input == "/quit" || input == "/exit" {
                            let _ = tx.send(input).await;
                            break;
                        }
                        if input == "/list" {
                            let guard = client_ref.read().await;
                            match guard.conversation().list().await {
                                Ok(list) => {
                                    info!("会话列表 ({}):", list.len());
                                    for c in list.iter().take(10) {
                                        info!("  {} 未读={}", c.conversation_id, c.unread_count);
                                    }
                                }
                                Err(e) => warn!("list 失败: {}", e),
                            }
                            continue;
                        }
                        if input == "/history" {
                            let guard = client_ref.read().await;
                            match guard.message().list(&conv_id, u64::MAX, 20).await {
                                Ok(msgs) => {
                                    info!("最近 {} 条:", msgs.len());
                                    for m in msgs.iter().take(10) {
                                        let preview = decode_content(m).map(|d| d.text_preview().to_string()).unwrap_or_else(|_| "[无法解码]".into());
                                        info!("  [{}] {}", m.sender_id, preview);
                                    }
                                }
                                Err(e) => warn!("history 失败: {}", e),
                            }
                            continue;
                        }
                        if input == "/read" {
                            let guard = client_ref.read().await;
                            if let Err(e) = guard.conversation().mark_read(&conv_id, u64::MAX).await {
                                warn!("标记已读失败: {}", e);
                            } else {
                                info!("已标记已读");
                            }
                            continue;
                        }
                        if let Err(e) = tx.send(input).await {
                            error!("发送到主循环失败: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("读取输入失败: {}", e);
                        break;
                    }
                }
            }
        })
    };

    let mut client_guard = client_tx.write().await;
    let mut retry_delay = tokio::time::Duration::from_millis(500);
    while let Some(text) = rx.recv().await {
        if text == "/quit" || text == "/exit" {
            break;
        }
        let msg = match MessageBuilder::new(&conversation_id, &my_user_id)
            .content(ContentBuilder::text(&text).build())
            .receiver(&chat_with)
            .single_chat()
            .build()
        {
            Ok(m) => m,
            Err(e) => {
                error!("构建消息失败: {}", e);
                continue;
            }
        };
        let mut tried = 0u32;
        loop {
            match client_guard.message().send(msg.clone()).await {
                Ok(ack) => {
                    if ack.success {
                        info!("已发送 seq={}", ack.seq);
                    } else {
                        warn!("发送未成功: server_msg_id={}", ack.server_msg_id);
                    }
                    break;
                }
                Err(e) => {
                    tried += 1;
                    if tried >= 5 {
                        error!("发送失败（已重试 5 次）: {}", e);
                        break;
                    }
                    warn!("发送失败，{}ms 后重试: {}", retry_delay.as_millis(), e);
                    tokio::time::sleep(retry_delay).await;
                    retry_delay = std::cmp::min(retry_delay * 2, tokio::time::Duration::from_secs(2));
                }
            }
        }
    }

    input_handle.abort();
    if let Err(e) = client_guard.disconnect().await {
        warn!("断开连接时出错: {}", e);
    }
    info!("已退出");
    Ok(())
}
