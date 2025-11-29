//! 一对一聊天客户端示例
//!
//! 一个交互式的一对一聊天客户端，支持：
//! - 手动指定当前用户ID（通过环境变量 MY_USER_ID）
//! - 指定聊天对象（通过环境变量 CHAT_WITH 或交互式输入）
//! - 创建单聊会话
//! - 发送和接收消息
//! - 实时显示消息
//!
//! ## 运行方式
//!
//! ```bash
//! # 基本运行（会提示输入聊天对象）
//! RUST_LOG=info cargo run --example two_clients_chat
//!
//! # 通过环境变量指定用户和聊天对象
//! RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob SERVER_URL=ws://localhost:60051/ws cargo run --example two_clients_chat
//!
//! # 只指定当前用户，聊天对象通过交互输入
//! RUST_LOG=info MY_USER_ID=user-alice cargo run --example two_clients_chat
//! ```
//! # 指定登录用户和对方用户
//! RUST_LOG=info MY_USER_ID=user-alice CHAT_WITH=user-bob cargo run --example two_clients_chat
//!
//! # 只指定登录用户，交互输入对方用户
//! RUST_LOG=info MY_USER_ID=user-alice cargo run --example two_clients_chat
//! ```

use flare_im_core_sdk::{
    FlareIMClient, ClientConfig,
    Event, ConnectionEvent, MessageEvent, SessionEvent,
};
use flare_core::common::config_types::TransportProtocol;
use std::collections::HashMap;
use tracing::{info, error, warn, debug};
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tokio::io::{self, AsyncBufReadExt};
use tokio::sync::mpsc;
use uuid::Uuid;

/// 客户端信息
struct ClientInfo {
    client: FlareIMClient,
    user_id: String,
    // 注意：event_rx 不能存储在结构体中，因为 mpsc::Receiver 不能 clone
    // 我们会在需要时从 event_bus 重新订阅
}

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("🚀 Flare IM SDK 一对一聊天客户端");
    info!("==========================================");

    // ============================================================
    // 1. 获取配置信息
    // ============================================================
    let server_url = std::env::var("SERVER_URL")
        .unwrap_or_else(|_| "ws://localhost:60051/ws".to_string());
    
    // 从环境变量获取当前用户ID
    let my_user_id = std::env::var("MY_USER_ID")
        .unwrap_or_else(|_| {
            // 如果没有环境变量，使用进程ID作为默认值
            format!("user-{}", std::process::id())
        });
    
    // 从环境变量获取聊天对象
    let chat_with_user_id = std::env::var("CHAT_WITH")
        .unwrap_or_else(|_| String::new());
    
    info!("📋 配置信息:");
    info!("   服务器地址: {}", server_url);
    info!("   当前用户ID: {}", my_user_id);
    
    // 如果未指定聊天对象，提示用户输入
    let chat_with = if chat_with_user_id.is_empty() {
        use tokio::io::{self, AsyncBufReadExt};
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
            // 如果 TOKEN 环境变量为空，生成测试 token
            generate_test_token(&my_user_id)?
        }
    } else {
        // 生成测试用的 JWT token（仅用于开发测试）
        // 注意：生产环境必须使用服务器颁发的有效 token
        generate_test_token(&my_user_id)?
    };
    
    // 优化：减少字符串克隆
    // 优化：减少字符串克隆
    let token_display = if token.len() > 50 { 
        format!("{}...", &token[..50]) 
    } else { 
        token.clone() 
    };
    info!("🔑 使用 Token (长度: {}): {}", token.len(), token_display);

    // ============================================================
    // 3. 创建客户端（使用生成的 token）
    // ============================================================
    info!("📦 创建客户端实例...");
    let client = Arc::new(create_client(&server_url, &my_user_id, &format!("device-{}", my_user_id), &token).await?);
    info!("✅ 客户端创建成功");

    // ============================================================
    // 4. 登录（使用相同的 token）
    // ============================================================
    info!("");
    info!("🔐 开始登录...");
    let login_result = client.client.login(&my_user_id, &token).await?;
    info!("✅ 登录成功: {:?}", login_result);
    
    // 等待连接稳定
    sleep(Duration::from_secs(2)).await;

    // ============================================================
    // 5. 创建或获取会话
    // ============================================================
    info!("");
    info!("💬 准备会话...");
    info!("   登录用户: {}", my_user_id);
    info!("   对方用户: {}", chat_with);
    
    // 尝试查找现有会话或创建新会话
    let session_id = match find_or_create_session(&client.client, &my_user_id, &chat_with).await {
        Ok(session_id) => {
            info!("✅ 会话准备完成: {}", session_id);
            session_id
        }
        Err(e) => {
            warn!("⚠️  会话准备失败: {}", e);
            info!("💡 提示: 可能服务器不支持创建会话，或需要等待对方先创建会话");
            info!("   程序将继续运行，等待接收消息...");
            // 使用标准化的会话ID
            generate_single_chat_session_id(&my_user_id, &chat_with)
        }
    };

    // ============================================================
    // 6. 启动消息接收任务
    // ============================================================
    let client_clone = client.clone();
    let session_id_clone = session_id.clone();
    let my_user_id_clone = my_user_id.clone();
    
    let message_receiver_task = tokio::spawn(async move {
        handle_incoming_messages(&client_clone, &session_id_clone, &my_user_id_clone).await;
    });

    // ============================================================
    // 6. 交互式消息发送
    // ============================================================
    info!("");
    info!("==========================================");
    info!("✅ 聊天客户端已就绪！");
    info!("   会话ID: {}", session_id);
    info!("   聊天对象: {}", chat_with);
    info!("");
    info!("💡 使用说明:");
    info!("   - 输入消息内容，按 Enter 发送");
    info!("   - 输入 '/exit' 或 '/quit' 退出");
    info!("   - 输入 '/help' 查看帮助");
    info!("==========================================");
    info!("");

    // 优化：预分配通道容量（根据实际需求调整）
    let (tx, mut rx) = mpsc::channel::<String>(100);
    
    // 启动输入读取任务
    let client_for_status = client.clone();
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
                            info!("");
                            info!("可用命令:");
                            info!("  /exit, /quit  - 退出聊天");
                            info!("  /help         - 显示帮助");
                            info!("  /status       - 显示连接状态");
                            info!("");
                            continue;
                        } else if input == "/status" {
                            let state = client_for_status.client.connection_state().await;
                            info!("连接状态: {:?}", state);
                            continue;
                        }
                        
                        // 发送消息
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
                        // 发送消息
                        send_message(
                            &client.client,
                            &session_id,
                            &my_user_id,
                            &text,
                        ).await;
                    }
                    None => {
                        // 输入通道关闭，退出
                        break;
                    }
                }
            }
            // 检查消息接收任务是否完成
            _ = tokio::signal::ctrl_c() => {
                info!("");
                info!("👋 收到退出信号，正在关闭...");
                break;
            }
        }
    }

    // 取消任务
    input_task.abort();
    message_receiver_task.abort();

    // ============================================================
    // 7. 优雅关闭
    // ============================================================
    info!("");
    info!("👋 正在关闭客户端...");
    
    if let Err(e) = client.client.logout().await {
        warn!("⚠️  登出失败: {}", e);
    } else {
        info!("✅ 已登出");
    }

    info!("✅ 客户端已关闭");
    Ok(())
}

/// 生成测试用的 JWT token
/// 
/// 注意：这仅用于开发测试，生产环境必须使用服务器颁发的有效 token
fn generate_test_token(user_id: &str) -> Result<String> {
    use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};
    use serde::{Deserialize, Serialize};
    use std::time::{SystemTime, UNIX_EPOCH};
    
    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String,  // user_id
        iss: String,  // issuer
        exp: usize,   // expiration time
        iat: usize,   // issued at
        jti: String,  // JWT ID
    }
    
    // 使用固定的 secret（与服务器配置一致）
    // 注意：生产环境必须使用服务器颁发的有效 token，不要使用此测试 token
    let secret = "insecure-secret";
    
    // 使用固定的 issuer（与服务器配置一致）
    let issuer = "flare-im-core";
    
    // 计算过期时间（7天后）
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    let exp = now + 7 * 24 * 60 * 60; // 7 days
    
    // 生成 JWT ID
    let jti = Uuid::new_v4().to_string();
    
    let claims = Claims {
        sub: user_id.to_string(),
        iss: issuer.to_string(),
        exp,
        iat: now,
        jti,
    };
    
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| anyhow::anyhow!("Failed to generate token: {}", e))?;
    
    Ok(token)
}

/// 创建客户端
async fn create_client(
    server_url: &str,
    user_id: &str,
    device_id: &str,
    token: &str,
) -> Result<ClientInfo> {
    
    // 优化：预分配 Vec 容量
    let protocols = vec![
        TransportProtocol::WebSocket,
        TransportProtocol::QUIC,
    ];
    
    // 优化：预分配 HashMap 容量
    let mut protocol_urls = HashMap::with_capacity(2);
    protocol_urls.insert(TransportProtocol::WebSocket, server_url.to_string());
    let quic_url = server_url.replace("ws://", "quic://").replace("/ws", "").replace("60051", "60052");
    protocol_urls.insert(TransportProtocol::QUIC, quic_url);
    
    let config = ClientConfig::builder()
        .server_url(server_url)
        .user_id(user_id)
        .device_id(device_id)
        .device_platform(flare_im_core_sdk::DevicePlatform::Desktop)
        .protocols(protocols)
        .protocol_urls(protocol_urls)
        .connect_timeout(30)  // 增加超时时间
        .race_timeout(Duration::from_secs(30))
        .heartbeat_interval(30)
        .auto_reconnect(true)
        .max_reconnect_attempts(10)
        .token(token)
        .build()?;
    
    let client = FlareIMClient::new(config).await?;
    
    Ok(ClientInfo {
        client,
        user_id: user_id.to_string(),
    })
}

/// 生成单聊会话ID（使用标准算法，确保两个用户生成相同的会话ID）
/// 
/// 使用 flare-core 的标准会话ID生成函数，通过排序两个用户ID并计算哈希，
/// 确保无论哪个用户创建会话，生成的会话ID都相同
/// 
/// 格式：`1-{32位十六进制哈希}`
fn generate_single_chat_session_id(user_id1: &str, user_id2: &str) -> String {
    // 使用 flare-core 的标准会话ID生成函数
    flare_core::common::session_id::generate_single_chat_session_id(user_id1, user_id2)
}

/// 查找或创建会话
async fn find_or_create_session(
    client: &FlareIMClient,
    my_user_id: &str,
    chat_with: &str,
) -> Result<String> {
    // 使用标准算法生成会话ID（确保两个用户使用相同的会话ID）
    // 格式：1-{32位十六进制哈希}，基于排序后的两个用户ID
    let session_id = generate_single_chat_session_id(my_user_id, chat_with);
    
    info!("🔍 查找会话: {}", session_id);
    info!("   登录用户: {}", my_user_id);
    info!("   对方用户: {}", chat_with);
    
    // 1. 先尝试查找现有会话
    let sessions = client.get_sessions(flare_im_core_sdk::SessionFilter::default()).await?;
    
    // 查找与目标用户的会话
    for session in &sessions {
        // 优先匹配标准会话ID
        if session.session_id == session_id {
            info!("✅ 找到现有会话: {}", session.session_id);
            return Ok(session.session_id.clone());
        }
        // 兼容旧格式：检查会话ID是否包含目标用户ID
        if session.session_id.contains(chat_with) && session.session_id.contains(my_user_id) {
            info!("✅ 找到现有会话（旧格式）: {}", session.session_id);
            return Ok(session.session_id.clone());
        }
    }
    
    // 2. 如果没有找到，创建新会话
    info!("💬 创建新会话...");
    info!("   会话ID: {}", session_id);
    info!("   会话类型: single (单聊)");
    info!("   参与者: {} <-> {}", my_user_id, chat_with);
    
    match client.create_session(
        Some(session_id.clone()),
        "single".to_string(),
        "chat".to_string(),
        Some(format!("Chat: {} <-> {}", my_user_id, chat_with)),
    ).await {
        Ok(created_session_id) => {
            info!("✅ 会话创建成功: {}", created_session_id);
            Ok(created_session_id)
        }
        Err(e) => {
            // 如果创建失败，使用生成的会话ID
            warn!("⚠️  会话创建失败: {}", e);
            info!("💡 使用生成的会话ID: {}", session_id);
            info!("   提示: 如果服务器不支持创建会话，将使用此会话ID等待对方创建");
            Ok(session_id)
        }
    }
}

/// 处理接收到的消息
async fn handle_incoming_messages(
    client: &Arc<ClientInfo>,
    session_id: &str,
    my_user_id: &str,
) {
    // 优化：使用 DashSet 替代 Mutex<HashSet>，减少锁竞争
    use dashmap::DashSet;
    use std::sync::Arc;
    
    let event_bus = client.client.event_bus();
    let mut event_rx = event_bus.subscribe();
    
    // 使用 DashSet 跟踪已处理的消息 ID，避免重复打印（无锁并发）
    let processed_messages: Arc<DashSet<String>> = Arc::new(DashSet::new());
    
    info!("📡 开始监听消息...");
    
    while let Ok(event) = event_rx.recv().await {
        match &event {
            Event::Connection(ConnectionEvent::Connected { protocol }) => {
                info!("✅ 连接成功，协议: {:?}", protocol);
            }
            Event::Connection(ConnectionEvent::Authenticated) => {
                info!("✅ 认证成功");
            }
            Event::Connection(ConnectionEvent::Disconnected) => {
                warn!("⚠️  连接断开");
            }
            Event::Connection(ConnectionEvent::Reconnecting) => {
                info!("🔄 正在重连...");
            }
            Event::Connection(ConnectionEvent::Reconnected) => {
                info!("✅ 重连成功");
            }
            Event::Message(MessageEvent::MessageReceived { message_id, session_id: msg_session_id }) => {
                if msg_session_id == session_id {
                    // 优化：使用 DashSet 的无锁检查，减少锁竞争
                    // 检查是否已经处理过这条消息
                    if processed_messages.contains(message_id) {
                        // 已经处理过，跳过
                        continue;
                    }
                    
                    // 标记为已处理（提前标记，避免并发重复处理）
                    // 优化：使用 DashSet 的 insert，返回 true 表示是新插入的
                    if !processed_messages.insert(message_id.clone()) {
                        // 如果插入失败（已存在），跳过
                        continue;
                    }
                    
                    // 等待一小段时间，确保消息已保存到本地存储
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    
                    // 获取消息内容（获取最新的消息列表，然后查找匹配的消息）
                    // 优化：使用更小的 limit，减少内存使用
                    if let Ok(messages) = client.client.get_messages(msg_session_id, 20, None).await {
                        // 优化：使用 HashMap 查找，O(1) 复杂度（如果消息数量大）
                        // 对于小数量（20条），直接迭代查找即可
                        if let Some(message) = messages.iter().find(|m| m.id == *message_id) {
                            // 检查是否是来自聊天对象的消息（不是自己发送的）
                            if message.sender_id != my_user_id {
                                // 过滤掉 Typing 消息，不显示
                                if message.message_type == flare_proto::MessageType::Typing as i32 {
                                    debug!("收到 Typing 消息，跳过显示: message_id={}", message_id);
                                    continue;
                                }
                                
                                // 显示接收到的消息
                                if let Some(content) = &message.content {
                                    match &content.content {
                                        Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) => {
                                            // 清理文本内容：移除控制字符（如 \x08 退格字符）和无效字符
                                            // 保留可打印字符和空白字符（空格、换行、制表符等）
                                            let cleaned_text: String = text_content.text
                                                .chars()
                                                .filter(|c| {
                                                    // 保留空白字符（空格、换行、制表符等）
                                                    if c.is_whitespace() {
                                                        true
                                                    } else {
                                                        // 过滤掉所有控制字符（包括 \x08 退格字符）
                                                        !c.is_control()
                                                    }
                                                })
                                                .collect();
                                            
                                            // 去除首尾空白
                                            let cleaned_text = cleaned_text.trim();
                                            
                                            // 如果清理后的文本为空，跳过显示
                                            if cleaned_text.is_empty() {
                                                debug!("收到空文本消息，跳过显示: message_id={}", message_id);
                                                continue;
                                            }
                                            
                                            info!("");
                                            info!("📨 收到新消息 [{}]:", message.sender_id);
                                            info!("   {}", cleaned_text);
                                            info!("");
                                        }
                                        Some(flare_proto::flare::common::v1::message_content::Content::Typing(_)) => {
                                            // Typing 消息不显示
                                            debug!("收到 Typing 消息，跳过显示: message_id={}", message_id);
                                        }
                                        _ => {
                                            info!("📨 收到新消息 [{}]: (非文本消息)", message.sender_id);
                                        }
                                    }
                                } else {
                                    // 如果没有 content，记录警告但不显示
                                    debug!("消息内容为空: message_id={}", message_id);
                                }
                            }
                        } else {
                            // 如果获取不到消息，记录调试信息
                            debug!("消息尚未保存到本地存储或已过期: message_id={}", message_id);
                        }
                    } else {
                        // 获取消息失败，记录错误但不影响其他消息处理
                        debug!("获取消息失败: message_id={}", message_id);
                    }
                }
            }
            Event::Message(MessageEvent::MessageSent { message_id: _, session_id: msg_session_id }) => {
                if msg_session_id == session_id {
                    // 消息发送成功，不显示详细信息（避免干扰聊天界面）
                    // 如果需要，可以在这里添加发送成功的提示
                }
            }
            Event::Session(SessionEvent::SessionCreated { session_id: sess_id }) => {
                if sess_id == session_id {
                    info!("💬 会话已创建: {}", sess_id);
                }
            }
            _ => {
                // 忽略其他事件
            }
        }
    }
}

/// 发送消息
async fn send_message(
    client: &FlareIMClient,
    session_id: &str,
    _sender_id: &str,
    text: &str,
) {
    let content = flare_proto::MessageContent {
        content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
            flare_proto::TextContent {
                text: text.to_string(),
                mentions: vec![],
            }
        )),
    };
    
    match client.send_message(session_id, content).await {
        Ok(_message_id) => {
            // 消息发送成功，不显示详细信息（避免干扰聊天界面）
        }
        Err(e) => {
            error!("❌ 消息发送失败: {}", e);
            info!("💡 提示: 请检查连接状态和会话ID是否正确");
        }
    }
}

