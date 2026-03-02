//! 一对一聊天客户端示例（使用 Interface API）
//!
//! 一个交互式的一对一聊天客户端，展示如何使用 Interface 层的所有 API：
//! - 使用事件订阅 API 监听消息
//! - 使用消息创建 API 创建各种类型的消息
//! - 使用会话管理 API 管理会话
//! - 使用扩展 API 注册自定义扩展
//! - 使用指标 API 监控消息发送
//!
//! ## 运行方式
//!
//! ```bash
//! # 基本运行（会提示输入聊天对象）
//! RUST_LOG=info cargo run --example two_clients_chat
//!
//! # 通过环境变量指定用户和聊天对象
//! RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob SERVER_URL=ws://localhost:60051 cargo run --example two_clients_chat
//!
//! # 只指定当前用户，聊天对象通过交互输入
//! RUST_LOG=info MY_USER_ID=123456 cargo run --example two_clients_chat
//! RUST_LOG=info MY_USER_ID=12345 cargo run --example two_clients_chat
//! ```

use anyhow::Result;
use flare_im_core_sdk::config::SdkConfig;
use flare_im_core_sdk::interface::facade::ImCoreSdk;
use flare_im_core_sdk_storage_sqlite::{create_storage, SqliteEventStore, SqliteMessageRepository, SqliteConversationRepository};

use flare_im_core_sdk::domain::event::subscribers::*;
use flare_im_core_sdk::domain::event::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// 客户端信息
struct ClientInfo {
    sdk: Arc<ImCoreSdk>,
    user_id: String,
    /// 当前聊天的接收者ID
    chat_with: String,
}

// 使用 flare-core 的标准会话 ID 生成函数
use flare_im_core_sdk::shared::utils::{generate_single_chat_conversation_id, generate_test_token};

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("🚀 Flare IM SDK 一对一聊天客户端（Interface API 版本）");
    info!("==========================================");

    // ============================================================
    // 1. 获取配置信息
    // ============================================================
    let server_url = std::env::var("SERVER_URL")
        .unwrap_or_else(|_| "ws://localhost:60051".to_string());

    // 从环境变量获取当前用户ID
    let my_user_id = std::env::var("MY_USER_ID").unwrap_or_else(|_| {
        format!("user-{}", std::process::id())
    });

    // 从环境变量获取聊天对象
    let chat_with_user_id = std::env::var("CHAT_WITH").unwrap_or_else(|_| String::new());

    info!("📋 配置信息:");
    info!("   服务器地址: {}", server_url);
    info!("   当前用户ID: {}", my_user_id);

    // 如果未指定聊天对象，提示用户输入
    let chat_with = if chat_with_user_id.is_empty() {
        info!("");
        info!("请输入要聊天的用户ID（按 Enter 确认）:");
        let mut input = String::new();
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin);
        reader.read_line(&mut input).await?;
        let chat_with = input.trim().to_string();

        if chat_with.is_empty() {
            return Err(anyhow::anyhow!("聊天对象不能为空"));
        }
        chat_with
    } else {
        chat_with_user_id
    };

    info!("   聊天对象: {}", chat_with);
    info!("");

    // ============================================================
    // 2. 生成或获取 token
    // ============================================================
    let token = if let Ok(env_token) = std::env::var("TOKEN") {
        if !env_token.is_empty() {
            env_token
        } else {
            generate_test_token(&my_user_id)?
        }
    } else {
        generate_test_token(&my_user_id)?
    };

    let token_display = if token.len() > 50 {
        format!("{}...", &token[..50])
    } else {
        token.clone()
    };
    info!("🔑 使用 Token (长度: {}): {}", token.len(), token_display);

    // ============================================================
    // 3. 创建 SDK 实例
    // ============================================================
    info!("📦 创建 SDK 实例...");
    
    // 按用户ID生成不同的DB文件路径，避免不同用户数据混淆
    let safe_user_id = my_user_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let storage_path = PathBuf::from(format!("./flare-im-{}", safe_user_id));
    
    info!("📦 使用用户专属存储: {}", storage_path.display());

    let config = SdkConfig::builder()
        .websocket_url(&server_url)
        .storage_path(&storage_path)
        .media_cache_path(storage_path.join("media_cache"))
        .log_level("info")
        .build();
    
    // 清理历史数据：删除现有的数据库文件以确保使用最新表结构
    let db_path = storage_path.join("flare_im.db");
    if db_path.exists() {
        std::fs::remove_file(&db_path).unwrap();
        info!("已删除现有数据库文件: {}", db_path.display());
    }
    
    // 确保存储目录存在
    std::fs::create_dir_all(&storage_path).unwrap();
    
    // 创建 SQLite 存储实现
    let database_url = format!("sqlite:{}?mode=rwc", db_path.to_string_lossy());
    let (event_store, message_repository, conversation_repository) = 
        create_storage(&database_url).await.unwrap();
    
    let sdk = Arc::new(ImCoreSdk::new(
        config,
        event_store as std::sync::Arc<dyn flare_im_core_sdk::domain::repository::EventStore>,
        message_repository as std::sync::Arc<dyn flare_im_core_sdk::domain::repository::MessageRepository>,
        conversation_repository as std::sync::Arc<dyn flare_im_core_sdk::domain::repository::ConversationRepository>,
    ).await?);
    info!("✅ SDK 创建成功");

    // ============================================================
    // 4. 注册事件订阅者（使用 Interface API）
    // ============================================================
    info!("");
    info!("📡 注册事件订阅者...");
    
    // 创建消息事件订阅者
    let message_subscriber = Arc::new(MessageEventHandler {
        user_id: my_user_id.clone(),
        chat_with: chat_with.clone(),
    });
    
    // 使用 Interface API 注册订阅者
    let events = sdk.events();
    let message_sub_id = events.subscribe_message(message_subscriber).await;
    info!("✅ 消息事件订阅者已注册: {}", message_sub_id);
    
    // 创建连接事件订阅者
    let connection_subscriber = Arc::new(ConnectionEventHandler);
    let connection_sub_id = events.subscribe_connection(connection_subscriber).await;
    info!("✅ 连接事件订阅者已注册: {}", connection_sub_id);
    
    // 创建会话事件订阅者
    let session_subscriber = Arc::new(SessionEventHandler);
    let session_sub_id = events.subscribe_session(session_subscriber).await;
    info!("✅ 会话事件订阅者已注册: {}", session_sub_id);
    
    // 创建同步事件订阅者
    let sync_subscriber = Arc::new(SyncEventHandler);
    let sync_sub_id = events.subscribe_sync(sync_subscriber).await;
    info!("✅ 同步事件订阅者已注册: {}", sync_sub_id);
    
    // 显示订阅统计信息
    let stats = events.get_statistics().await;
    info!("📊 订阅统计: 消息={}, 连接={}, 会话={}, 同步={}", 
        stats.message_subscribers, 
        stats.connection_subscribers,
        stats.session_subscribers,
        stats.sync_subscribers);

    let client = ClientInfo {
        sdk: sdk.clone(),
        user_id: my_user_id.clone(),
        chat_with: chat_with.clone(),
    };

    // ============================================================
    // 5. 建立真实连接并完成同步
    // ============================================================
    info!("");
    info!("🔐 开始登录并建立连接...");
    establish_real_connection(&sdk, &my_user_id).await
        .expect("必须连接到服务端才能运行示例");
    info!("✅ 连接和同步完成");

    // ============================================================
    // 7. 生成会话ID
    // ============================================================
    info!("");
    info!("💬 准备会话...");
    info!("   登录用户: {}", my_user_id);
    info!("   对方用户: {}", chat_with);

    let conversation_id = generate_single_chat_conversation_id(&my_user_id, &chat_with);
    info!("✅ 会话ID: {}", conversation_id);

    // ============================================================
    // 8. 查询会话信息（使用 Interface API）
    // ============================================================
    info!("");
    info!("📋 查询会话信息...");
    
    // 获取会话详情
    if let Ok(conversation) = sdk.conversation().get_one_conversation(conversation_id.clone()).await {
        info!("✅ 会话已存在: {}", conversation.conversation_id);
        info!("   未读数: {}", conversation.unread_count);
    } else {
        info!("ℹ️  会话尚未创建（将在首次发送消息时创建）");
    }
    
    // 获取总未读数
    if let Ok(total_unread) = sdk.conversation().get_total_unread_msg_count().await {
        info!("📊 总未读数: {}", total_unread);
    }
    
    // 获取所有会话列表
    if let Ok(conversations) = sdk.conversation().get_all_conversation_list().await {
        info!("📋 会话总数: {}", conversations.len());
    }

    // ============================================================
    // 9. 查询历史消息（使用 Interface API）
    // ============================================================
    info!("");
    info!("📜 查询历史消息...");
    
    // 使用 find_message_list 查询消息
    if let Ok(messages) = sdk.message().find_message_list(
        Some(conversation_id.clone()),
        None,
        None,
        None,
        Some(10),
    ).await {
        info!("📜 最近消息数: {}", messages.len());
        for (idx, msg) in messages.iter().take(3).enumerate() {
            let sender_id = &msg.sender_id;
            // 提取文本内容
            use flare_proto::MessageContentExt;
            let content_text = if let Ok(content) = flare_proto::decode_message_content(&msg.content) {
                if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = content.content {
                    text_content.text
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            info!("   {}. [{}]: {}", idx + 1, sender_id, content_text);
        }
    }

    // ============================================================
    // 10. 交互式消息发送
    // ============================================================
    info!("");
    info!("==========================================");
    info!("✅ 聊天客户端已就绪！");
    info!("   会话ID: {}", conversation_id);
    info!("   聊天对象: {}", chat_with);
    info!("");
    info!("💡 使用说明:");
    info!("   - 输入消息内容，按 Enter 发送");
    info!("   - 输入 '/exit' 或 '/quit' 退出");
    info!("   - 输入 '/help' 查看帮助");
    info!("   - 输入 '/image <url>' 发送图片消息");
    info!("   - 输入 '/audio <url> <duration>' 发送语音消息");
    info!("   - 输入 '/file <url> <name>' 发送文件消息");
    info!("   - 输入 '/location <lng> <lat> <address>' 发送位置消息");
    info!("   - 输入 '/card <user_id> <name> <avatar>' 发送名片消息");
    info!("   - 输入 '/custom <type> <data>' 发送自定义消息");
    info!("   - 输入 '/metrics' 查看消息指标");
    info!("   - 输入 '/queue' 查看消息队列状态");
    info!("==========================================");
    info!("");

    let (tx, mut rx) = mpsc::channel::<String>(100);

    // 启动输入读取任务
    let client_for_input = client.clone();
    let conversation_id_for_input = conversation_id.clone();
    let my_user_id_for_input = my_user_id.clone();
    let input_task = {
        let tx = tx.clone();
        tokio::spawn(async move {
            let stdin = io::stdin();
            let mut reader = io::BufReader::new(stdin);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let input = line.trim().to_string();
                        if input.is_empty() {
                            continue;
                        }

                        // 处理命令
                        if input == "/exit" || input == "/quit" {
                            info!("👋 退出聊天...");
                            break;
                        } else if input == "/help" {
                            show_help();
                            continue;
                        } else if input == "/read" {
                            // 标记当前会话已读
                            if let Err(e) = client_for_input
                                .sdk
                                .conversation()
                                .mark_conversation_message_as_read(
                                    conversation_id_for_input.clone(),
                                )
                                .await
                            {
                                warn!("标记已读失败: {}", e);
                            } else {
                                info!("✅ 已标记会话为已读");
                            }
                            continue;
                        } else if input == "/sessions" {
                            // 显示会话列表
                            show_conversations(&client_for_input.sdk).await;
                            continue;
                        } else if input == "/history" {
                            // 显示最近消息历史
                            show_message_history(&client_for_input.sdk, &conversation_id_for_input).await;
                            continue;
                        } else if input.starts_with("/image ") {
                            // 发送图片消息
                            let url = input[7..].trim().to_string();
                            if url.is_empty() {
                                warn!("请提供图片 URL");
                                continue;
                            }
                            send_image_message(&client_for_input.sdk, &conversation_id_for_input, &my_user_id_for_input, &client_for_input.chat_with, &url).await;
                            continue;
                        } else if input.starts_with("/audio ") {
                            // 发送语音消息
                            let parts: Vec<&str> = input[7..].trim().split_whitespace().collect();
                            if parts.len() < 2 {
                                warn!("用法: /audio <url> <duration_seconds>");
                                continue;
                            }
                            if let Ok(duration) = parts[1].parse::<u64>() {
                                send_audio_message(&client_for_input.sdk, &conversation_id_for_input, &my_user_id_for_input, &client_for_input.chat_with, parts[0], duration).await;
                            } else {
                                warn!("无效的时长");
                            }
                            continue;
                        } else if input.starts_with("/file ") {
                            // 发送文件消息
                            let parts: Vec<&str> = input[6..].trim().split_whitespace().collect();
                            if parts.len() < 2 {
                                warn!("用法: /file <url> <filename>");
                                continue;
                            }
                            send_file_message(&client_for_input.sdk, &conversation_id_for_input, &my_user_id_for_input, &client_for_input.chat_with, parts[0], parts[1]).await;
                            continue;
                        } else if input.starts_with("/location ") {
                            // 发送位置消息
                            let parts: Vec<&str> = input[10..].trim().split_whitespace().collect();
                            if parts.len() < 3 {
                                warn!("用法: /location <lng> <lat> <address>");
                                continue;
                            }
                            if let (Ok(lng), Ok(lat)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                                let address = parts[2..].join(" ");
                                send_location_message(&client_for_input.sdk, &conversation_id_for_input, &my_user_id_for_input, &client_for_input.chat_with, lng, lat, &address).await;
                            } else {
                                warn!("无效的经纬度");
                            }
                            continue;
                        } else if input.starts_with("/card ") {
                            // 发送名片消息
                            let parts: Vec<&str> = input[6..].trim().split_whitespace().collect();
                            if parts.len() < 3 {
                                warn!("用法: /card <user_id> <name> <avatar_url>");
                                continue;
                            }
                            send_card_message(&client_for_input.sdk, &conversation_id_for_input, &my_user_id_for_input, &client_for_input.chat_with, parts[0], parts[1], parts[2]).await;
                            continue;
                        } else if input.starts_with("/custom ") {
                            // 发送自定义消息
                            let parts: Vec<&str> = input[8..].trim().split_whitespace().collect();
                            if parts.len() < 2 {
                                warn!("用法: /custom <type> <data>");
                                continue;
                            }
                            let data = parts[1..].join(" ");
                            send_custom_message(&client_for_input.sdk, &conversation_id_for_input, &my_user_id_for_input, &client_for_input.chat_with, parts[0], &data).await;
                            continue;
                        } else if input == "/metrics" {
                            // 显示消息指标
                            show_metrics(&client_for_input.sdk).await;
                            continue;
                        } else if input == "/queue" {
                            // 显示消息队列状态
                            show_queue_status(&client_for_input.sdk).await;
                            continue;
                        } else if input == "/draft" {
                            // 设置会话草稿
                            info!("请输入草稿内容（按 Enter 确认，留空清空草稿）:");
                            let mut draft_input = String::new();
                            reader.read_line(&mut draft_input).await.ok();
                            let draft = draft_input.trim().to_string();
                            if draft.is_empty() {
                                if let Err(e) = client_for_input.sdk.conversation().set_conversation_draft(conversation_id_for_input.clone(), None).await {
                                    warn!("清空草稿失败: {}", e);
                                } else {
                                    info!("✅ 已清空草稿");
                                }
                            } else {
                                if let Err(e) = client_for_input.sdk.conversation().set_conversation_draft(conversation_id_for_input.clone(), Some(draft)).await {
                                    warn!("设置草稿失败: {}", e);
                                } else {
                                    info!("✅ 已设置草稿");
                                }
                            }
                            continue;
                        } else if input == "/search" {
                            // 搜索消息
                            info!("请输入搜索关键词:");
                            let mut keyword_input = String::new();
                            reader.read_line(&mut keyword_input).await.ok();
                            let keyword = keyword_input.trim().to_string();
                            if !keyword.is_empty() {
                                search_messages(&client_for_input.sdk, &conversation_id_for_input, &keyword).await;
                            }
                            continue;
                        }

                        // 发送普通文本消息
                        if let Err(e) = tx.send(input).await {
                            error!("发送消息到队列失败: {}", e);
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

    // 消息发送循环
    loop {
        tokio::select! {
            // 接收用户输入
            msg = rx.recv() => {
                match msg {
                    Some(text) => {
                        send_text_message(
                            &client.sdk,
                            &conversation_id,
                            &my_user_id,
                            &chat_with,
                            &text,
                        ).await;
                    }
                    None => {
                        break;
                    }
                }
            }
            // 检查退出信号
            _ = tokio::signal::ctrl_c() => {
                info!("");
                info!("👋 收到退出信号，正在关闭...");
                break;
            }
        }
    }

    // 取消任务
    input_task.abort();

    // ============================================================
    // 11. 取消事件订阅（使用 Interface API）
    // ============================================================
    info!("");
    info!("📡 取消事件订阅...");
    let events = sdk.events();
    events.unsubscribe_message(&message_sub_id).await;
    events.unsubscribe_connection(&connection_sub_id).await;
    events.unsubscribe_session(&session_sub_id).await;
    events.unsubscribe_sync(&sync_sub_id).await;
    info!("✅ 已取消所有事件订阅");

    // ============================================================
    // 12. 优雅关闭
    // ============================================================
    info!("");
    info!("👋 正在关闭客户端...");

    if let Err(e) = sdk.logout().await {
        warn!("⚠️  登出失败: {}", e);
    } else {
        info!("✅ 已登出");
    }

    info!("✅ 客户端已关闭");
    Ok(())
}

// ============================================================================
// 事件订阅者实现（使用 Interface API 的 Trait）
// ============================================================================

/// 消息事件处理器
struct MessageEventHandler {
    user_id: String,
    chat_with: String,
}

#[async_trait::async_trait]
impl MessageEventSubscriber for MessageEventHandler {
    async fn on_message_created(&self, event: &MessageCreated) -> anyhow::Result<()> {
        // 只显示来自聊天对象的消息（对方发来的）
        if event.sender_id != self.chat_with {
            return Ok(());
        }
        // 从 content 中提取文本内容（DefaultMessageHandler 存的是 message.content 即 Vec<u8> 的 JSON 数组）
        let content_str = if let Some(s) = event.content.as_str() {
            s.to_string()
        } else if let Some(arr) = event.content.as_array() {
            let bytes_vec: Result<Vec<u8>, _> = arr
                .iter()
                .map(|b| b.as_u64().ok_or(()))
                .map(|r| r.map(|u| u as u8))
                .collect();
            if let Ok(bytes) = bytes_vec {
                match flare_proto::decode_message_content(&bytes) {
                    Ok(decoded) => {
                        if let Some(flare_proto::flare::common::v1::message_content::Content::Text(t)) = decoded.content {
                            t.text
                        } else {
                            "[非文本消息]".to_string()
                        }
                    }
                    Err(_) => "[内容解码失败]".to_string(),
                }
            } else {
                "[内容格式异常]".to_string()
            }
        } else {
            "[未知格式]".to_string()
        };

        info!("");
        info!("📨 [{}] 发送了新消息:", event.sender_id);
        info!("   {}", content_str);
        info!("");
        Ok(())
    }

    async fn on_message_sent(&self, event: &MessageSent) -> anyhow::Result<()> {
        info!("✅ 消息已发送: {}", event.message_id);
        Ok(())
    }

    async fn on_message_send_failed(&self, event: &MessageSendFailed) -> anyhow::Result<()> {
        warn!("❌ 消息发送失败: {} - {}", event.message_id, event.error);
        Ok(())
    }

    async fn on_message_delivered(&self, event: &MessageDelivered) -> anyhow::Result<()> {
        // MessageDelivered 只有 message_id，需要通过消息ID查询消息详情
        // 这里简化处理，只记录消息ID
        debug!("消息已送达: {}", event.message_id);
        // 注意：实际应用中，可以通过 message_id 查询消息详情
        // 或者监听 MessageCreated 事件来获取完整的消息信息
        Ok(())
    }

    async fn on_message_read(&self, event: &MessageRead) -> anyhow::Result<()> {
        debug!("消息已读: {}", event.message_id);
        Ok(())
    }

    async fn on_message_recalled(&self, event: &MessageRecalled) -> anyhow::Result<()> {
        info!("⚠️  消息已撤回: {}", event.message_id);
        Ok(())
    }

    async fn on_message_edited(&self, _event: &MessageEdited) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_deleted(&self, _event: &MessageDeleted) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_reaction_added(&self, _event: &MessageReactionAdded) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_reaction_removed(&self, _event: &MessageReactionRemoved) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_pinned(&self, _event: &MessagePinned) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_unpinned(&self, _event: &MessageUnpinned) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_favorited(&self, _event: &MessageFavorited) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_unfavorited(&self, _event: &MessageUnfavorited) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_marked(&self, _event: &MessageMarked) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_unmarked(&self, _event: &MessageUnmarked) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_forwarded(&self, _event: &MessageForwarded) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_message_replied(&self, _event: &MessageReplied) -> anyhow::Result<()> {
        Ok(())
    }
}

/// 连接事件处理器
struct ConnectionEventHandler;

#[async_trait::async_trait]
impl ConnectionEventSubscriber for ConnectionEventHandler {
    async fn on_connected(&self, _event: &ConnectionConnected) -> anyhow::Result<()> {
        info!("🔗 连接已建立");
        Ok(())
    }

    async fn on_disconnected(&self, _event: &ConnectionDisconnected) -> anyhow::Result<()> {
        warn!("🔌 连接已断开");
        Ok(())
    }

    async fn on_reconnecting(&self, _event: &ConnectionReconnecting) -> anyhow::Result<()> {
        info!("🔄 正在重连...");
        Ok(())
    }

    async fn on_reconnected(&self, _event: &ConnectionReconnected) -> anyhow::Result<()> {
        info!("✅ 重连成功");
        Ok(())
    }

    async fn on_connect_failed(&self, event: &ConnectionConnectFailed) -> anyhow::Result<()> {
        warn!("❌ 连接失败: {}", event.error);
        Ok(())
    }
}

/// 会话事件处理器
struct SessionEventHandler;

#[async_trait::async_trait]
impl SessionEventSubscriber for SessionEventHandler {
    async fn on_logged_in(&self, _event: &SessionLoggedIn) -> anyhow::Result<()> {
        info!("✅ 已登录");
        Ok(())
    }

    async fn on_logged_out(&self, _event: &SessionLoggedOut) -> anyhow::Result<()> {
        info!("👋 已登出");
        Ok(())
    }

    async fn on_expired(&self, _event: &SessionExpired) -> anyhow::Result<()> {
        warn!("⚠️  会话已过期");
        Ok(())
    }

    async fn on_token_refreshed(&self, _event: &SessionTokenRefreshed) -> anyhow::Result<()> {
        info!("🔄 Token 已刷新");
        Ok(())
    }
}

/// 同步事件处理器
struct SyncEventHandler;

#[async_trait::async_trait]
impl SyncEventSubscriber for SyncEventHandler {
    async fn on_bootstrap_started(&self, _event: &SyncBootstrapStarted) -> anyhow::Result<()> {
        info!("🔄 Bootstrap Sync 开始");
        Ok(())
    }

    async fn on_bootstrap_completed(&self, _event: &SyncBootstrapCompleted) -> anyhow::Result<()> {
        info!("✅ Bootstrap Sync 完成");
        Ok(())
    }

    async fn on_bootstrap_failed(&self, event: &SyncBootstrapFailed) -> anyhow::Result<()> {
        warn!("❌ Bootstrap Sync 失败: {}", event.error);
        Ok(())
    }

    async fn on_async_started(&self, _event: &SyncAsyncStarted) -> anyhow::Result<()> {
        debug!("🔄 Async Sync 开始");
        Ok(())
    }

    async fn on_async_completed(&self, _event: &SyncAsyncCompleted) -> anyhow::Result<()> {
        debug!("✅ Async Sync 完成");
        Ok(())
    }

    async fn on_async_failed(&self, event: &SyncAsyncFailed) -> anyhow::Result<()> {
        warn!("❌ Async Sync 失败: {}", event.error);
        Ok(())
    }

    async fn on_progress_updated(&self, _event: &SyncProgressUpdated) -> anyhow::Result<()> {
        Ok(())
    }
}

// ============================================================================
// 辅助函数（使用 Interface API）
// ============================================================================

/// 显示帮助信息
fn show_help() {
    info!("");
    info!("可用命令:");
    info!("  /exit, /quit          - 退出聊天");
    info!("  /help                 - 显示帮助");
    info!("  /read                 - 标记当前会话已读");
    info!("  /sessions             - 显示会话列表");
    info!("  /history              - 显示最近消息历史");
    info!("  /image <url>          - 发送图片消息");
    info!("  /audio <url> <sec>    - 发送语音消息");
    info!("  /file <url> <name>    - 发送文件消息");
    info!("  /location <lng> <lat> <addr> - 发送位置消息");
    info!("  /card <uid> <name> <avatar> - 发送名片消息");
    info!("  /custom <type> <data> - 发送自定义消息");
    info!("  /metrics              - 查看消息指标");
    info!("  /queue               - 查看消息队列状态");
    info!("  /draft               - 设置/清空会话草稿");
    info!("  /search               - 搜索消息");
    info!("");
}

/// 显示会话列表
async fn show_conversations(sdk: &ImCoreSdk) {
    if let Ok(conversations) = sdk.conversation().get_all_conversation_list().await {
        info!("");
        info!("📋 会话列表 (共 {} 个):", conversations.len());
        for (idx, conv) in conversations.iter().enumerate() {
            let conv_id = &conv.conversation_id;
            let unread = conv.unread_count;
            info!("  {}. {} - 未读: {}", idx + 1, conv_id, unread);
        }
        info!("");
    } else {
        warn!("获取会话列表失败");
    }
}

/// 显示消息历史
async fn show_message_history(sdk: &ImCoreSdk, conversation_id: &str) {
    if let Ok(messages) = sdk.message().find_message_list(
        Some(conversation_id.to_string()),
        None,
        None,
        None,
        Some(20),
    ).await {
        info!("");
        info!("📜 最近消息历史 (共 {} 条):", messages.len());
        for msg in messages.iter().rev() {
            let sender_id = &msg.sender_id;
            // 提取文本内容
            use flare_proto::MessageContentExt;
            let content_text = if let Ok(content) = flare_proto::decode_message_content(&msg.content) {
                if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = content.content {
                    text_content.text
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            info!("  {}: {}", sender_id, content_text);
        }
        info!("");
    } else {
        warn!("获取消息历史失败");
    }
}

/// 显示消息指标
async fn show_metrics(sdk: &ImCoreSdk) {
    let metrics = sdk.get_message_metrics().await;
    info!("");
    info!("📊 消息指标:");
    info!("   总发送数: {}", metrics.sent_total);
    info!("   成功数: {}", metrics.sent_success);
    info!("   失败数: {}", metrics.sent_failed);
    info!("   ACK超时: {}", metrics.ack_timeout);
    info!("   平均延迟: {}ms", metrics.avg_send_latency_ms);
    info!("   成功率: {:.2}%", metrics.success_rate * 100.0);
    info!("");
}

/// 显示消息队列状态
async fn show_queue_status(sdk: &ImCoreSdk) {
    let queue = sdk.message_queue();
    let len = queue.len().await;
    let is_empty = queue.is_empty().await;
    info!("");
    info!("📬 消息队列状态:");
    info!("   队列长度: {}", len);
    info!("   是否为空: {}", is_empty);
    info!("");
}

/// 搜索消息
async fn search_messages(sdk: &ImCoreSdk, conversation_id: &str, keyword: &str) {
    if let Ok(messages) = sdk.message().search_local_messages(
        Some(conversation_id.to_string()),
        keyword.to_string(),
        Some(10),
    ).await {
        info!("");
        info!("🔍 搜索结果 (共 {} 条):", messages.len());
        for (idx, msg) in messages.iter().enumerate() {
            // 提取文本内容
            use flare_proto::MessageContentExt;
            let content_text = if let Ok(content) = flare_proto::decode_message_content(&msg.content) {
                if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = content.content {
                    text_content.text
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            if !content_text.is_empty() {
                info!("  {}. [{}]: {}", idx + 1, msg.sender_id, content_text);
            }
        }
        info!("");
    } else {
        warn!("搜索消息失败");
    }
}

/// 发送文本消息
async fn send_text_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    receiver_id: &str,
    text: &str,
) {
    // 创建文本消息
    let message = match sdk.message().create_text_message(
        text.to_string(),
        Some(receiver_id.to_string()),
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑，处理连接不稳定问题）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                debug!("✅ 消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// 发送图片消息
async fn send_image_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    _receiver_id: &str,
    image_url: &str,
) {
    // 创建图片消息 - 使用文本消息模拟
    let message = match sdk.message().create_text_message(
        format!("[图片消息: {}]", image_url),
        None,
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建图片消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                info!("✅ 图片消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 图片消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// 发送语音消息
async fn send_audio_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    _receiver_id: &str,
    audio_url: &str,
    duration_sec: u64,
) {
    // 创建语音消息 - 使用文本消息模拟
    let message = match sdk.message().create_text_message(
        format!("[语音消息: {}, 时长: {}秒]", audio_url, duration_sec),
        None,
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建语音消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                info!("✅ 语音消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 语音消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// 发送文件消息
async fn send_file_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    _receiver_id: &str,
    file_url: &str,
    file_name: &str,
) {
    // 创建文件消息 - 使用文本消息模拟
    let message = match sdk.message().create_text_message(
        format!("[文件消息: {}, 名称: {}]", file_url, file_name),
        Some("user_test_002".to_string()), // receiver_id
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建文件消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                info!("✅ 文件消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 文件消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// 发送位置消息
async fn send_location_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    _receiver_id: &str,
    longitude: f64,
    latitude: f64,
    address: &str,
) {
    // 创建位置消息 - 使用文本消息模拟
    let message = match sdk.message().create_text_message(
        format!("[位置消息: 经度={}, 纬度={}, 地址={}]", longitude, latitude, address),
        Some("user_test_002".to_string()), // receiver_id
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建位置消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                info!("✅ 位置消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 位置消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// 发送名片消息
async fn send_card_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    _receiver_id: &str,
    card_user_id: &str,
    card_name: &str,
    card_avatar: &str,
) {
    // 创建名片消息 - 使用文本消息模拟
    let message = match sdk.message().create_text_message(
        format!("[名片消息: 用户={}, 姓名={}, 头像={}]", card_user_id, card_name, card_avatar),
        Some("user_test_002".to_string()), // receiver_id
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建名片消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                info!("✅ 名片消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 名片消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}

/// 发送自定义消息
async fn send_custom_message(
    sdk: &ImCoreSdk,
    conversation_id: &str,
    sender_id: &str,
    _receiver_id: &str,
    business_type: &str,
    data: &str,
) {
    // 创建自定义消息 - 使用文本消息模拟
    let message = match sdk.message().create_text_message(
        format!("[自定义消息: 类型={}, 数据={}]", business_type, data),
        Some("user_test_002".to_string()), // receiver_id
    ).await {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建自定义消息失败: {}", e);
            return;
        }
    };

    // 发送消息（增加重试逻辑）
    let mut retry_count = 0;
    let max_retries = 5;
    loop {
        match sdk.message().send_message(message.clone(), conversation_id.to_string()).await {
            Ok(_) => {
                info!("✅ 自定义消息发送成功");
                break;
            }
            Err(e) => {
                retry_count += 1;
                if retry_count >= max_retries {
                    error!("❌ 自定义消息发送失败（重试 {} 次）: {}", max_retries, e);
                    return;
                }
                // 如果是 Sync is not Ready 错误，等待更长时间
                if e.to_string().contains("Sync is not Ready") {
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else if e.to_string().contains("user_id is unknown") {
                    // 如果是 user_id 未识别，等待连接稳定
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                } else {
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }
}


impl Clone for ClientInfo {
    fn clone(&self) -> Self {
        Self {
            sdk: self.sdk.clone(),
            user_id: self.user_id.clone(),
            chat_with: self.chat_with.clone(),
        }
    }
}

/// 建立真实连接并完成同步
/// 
/// 所有示例都需要服务端运行，如果连接失败，示例会失败
/// 连接后会执行 bootstrap_sync，确保可以发送消息
async fn establish_real_connection(sdk: &ImCoreSdk, user_id: &str) -> anyhow::Result<()> {
    // 使用与 interface_api_test.rs 完全相同的 token 生成方式
    let token = generate_test_token(user_id)?;
    
    // 登录（登录成功后会自动连接）
    sdk.login(user_id.to_string(), token).await
        .map_err(|e| anyhow::anyhow!("登录失败: {}。请确保服务端已启动并配置正确", e))?;
    
    // 等待连接稳定
    // 轮询检查连接状态，确保连接真正建立
    let max_wait_ms = 15000;
    let check_interval_ms = 200;
    let start_time = tokio::time::Instant::now();
    
    loop {
        // 检查连接状态（通过 SdkContext）
        if sdk.sdk_context().is_connected().await {
            // 连接已建立，额外等待一小段时间确保认证完成
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            break;
        }
        
        if start_time.elapsed().as_millis() > max_wait_ms as u128 {
            return Err(anyhow::anyhow!(
                "连接超时：在 {}ms 内未能建立连接。请检查服务端是否正常运行",
                max_wait_ms
            ));
        }
        
        tokio::time::sleep(tokio::time::Duration::from_millis(check_interval_ms)).await;
    }
    
    // 执行 Bootstrap Sync（必须成功，否则无法发送消息）
    sdk.bootstrap_sync().await.map_err(|e| {
        let msg = e.to_string();
        if msg.contains("6000") || msg.contains("Failed to load user conversations") {
            anyhow::anyhow!(
                "会话同步失败（服务端会话服务或数据库异常）: {}。请确认 flare-im-core 的 conversation 服务已启动且数据库已迁移（conversations / conversation_participants 表存在）",
                e
            )
        } else {
            anyhow::anyhow!("同步失败: {}。请确保服务端正常运行", e)
        }
    })?;
    
    // 等待 Sync 状态变为 Ready（确保可以发送消息）
    // 轮询检查 sync 状态，最多等待 10 秒
    let max_wait_ms = 10000;
    let check_interval_ms = 200;
    let start_time = tokio::time::Instant::now();
    
    loop {
        // 检查 sync 状态（通过 SdkContext 的 FSM）
        use flare_im_core_sdk::domain::sync::SyncState;
        let sync_state = sdk.sdk_context().fsm.sync_state().await;
        if sync_state == SyncState::Ready {
            // Sync 已就绪，额外等待一小段时间确保状态稳定
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            break;
        }
        
        if start_time.elapsed().as_millis() > max_wait_ms as u128 {
            return Err(anyhow::anyhow!(
                "Sync 状态超时：在 {}ms 内未能变为 Ready（当前状态: {:?}）。请检查服务端是否正常运行",
                max_wait_ms,
                sync_state
            ));
        }
        
        tokio::time::sleep(tokio::time::Duration::from_millis(check_interval_ms)).await;
    }
    
    Ok(())
}
