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

use anyhow::Result;
use flare_core::common::config_types::TransportProtocol;
use flare_core::common::session_id::generate_single_chat_session_id;
use flare_im_core_sdk::application::vo::{MessageVO, SessionVO};
use flare_im_core_sdk::domain::MessageBuilder;
use flare_im_core_sdk::{
    ClientConfig, ConnectionEvent, Event, FlareIMClient, MessageEvent, SessionEvent,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt};
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
/// 客户端信息
struct ClientInfo {
    client: FlareIMClient,
    user_id: String,
    /// 当前聊天的接收者ID（用于构建消息时设置 receiver_id）
    chat_with: String,
    // 注意：event_rx 不能存储在结构体中，因为 mpsc::Receiver 不能 clone
    // 我们会在需要时从 event_bus 重新订阅
}

/// 将消息类型数字转换为中文描述
fn message_type_to_chinese(message_type: i32) -> &'static str {
    match message_type {
        1 => "文本消息",
        2 => "图片消息",
        3 => "视频消息",
        4 => "语音消息",
        5 => "文件消息",
        6 => "位置消息",
        7 => "名片消息",
        8 => "自定义消息",
        9 => "通知消息",
        10 => "正在输入",
        11 => "撤回消息",
        12 => "已读消息",
        13 => "转发消息",
        14 => "投票消息",
        15 => "任务消息",
        16 => "日程消息",
        17 => "群公告消息",
        18 => "小程序消息",
        19 => "链接卡片",
        20 => "引用消息",
        21 => "话题消息",
        22 => "合并转发",
        _ => "未知消息类型",
    }
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
    let server_url =
        std::env::var("SERVER_URL").unwrap_or_else(|_| "ws://localhost:60051/ws".to_string());

    // 从环境变量获取当前用户ID
    let my_user_id = std::env::var("MY_USER_ID").unwrap_or_else(|_| {
        // 如果没有环境变量，使用进程ID作为默认值
        format!("user-{}", std::process::id())
    });

    // 从环境变量获取聊天对象
    let chat_with_user_id = std::env::var("CHAT_WITH").unwrap_or_else(|_| String::new());

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

        // 验证输入：如果输入看起来像命令，给出提示
        if chat_with.starts_with("RUST_LOG")
            || chat_with.contains("cargo run")
            || chat_with.contains("MY_USER_ID")
        {
            warn!("⚠️  检测到可能的命令输入，请重新输入用户ID");
            return Err(anyhow::anyhow!(
                "无效的用户ID输入，请重新运行程序并输入正确的用户ID"
            ));
        }
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
    let client = Arc::new(
        create_client(
            &server_url,
            &my_user_id,
            &format!("device-{}", my_user_id),
            &token,
            &chat_with, // 传递聊天对象ID，用于设置receiver_id
        )
        .await?,
    );
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

    let session_id_for_receiver = session_id.clone();
    let message_receiver_task = tokio::spawn(async move {
        handle_incoming_messages(&client_clone, &session_id_for_receiver, &my_user_id_clone).await;
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
    let session_id_for_input = session_id.clone();
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
                            info!("");
                            info!("可用命令:");
                            info!("  /exit, /quit  - 退出聊天");
                            info!("  /help         - 显示帮助");
                            info!("  /status       - 显示连接状态");
                            info!("  /read         - 标记当前会话已读");
                            info!("  /sessions     - 显示会话列表");
                            info!("  /history      - 显示最近消息历史");
                            info!("");
                            continue;
                        } else if input == "/status" {
                            let state = client_for_status.client.connection_state().await;
                            info!("连接状态: {:?}", state);
                            continue;
                        } else if input == "/read" {
                            // 标记当前会话已读
                            use flare_im_core_sdk::api::traits::SessionApi;
                            if let Err(e) = client_for_status
                                .client
                                .mark_read(&session_id_for_input, None)
                                .await
                            {
                                warn!("标记已读失败: {}", e);
                            } else {
                                info!("✅ 已标记会话为已读");
                            }
                            continue;
                        } else if input == "/sessions" {
                            // 显示会话列表
                            use flare_im_core_sdk::api::traits::SessionApi;
                            if let Ok(sessions) = client_for_status
                                .client
                                .get_sessions(flare_im_core_sdk::SessionFilter::default())
                                .await
                            {
                                info!("");
                                info!("📋 会话列表 (共 {} 个):", sessions.len());
                                for (idx, session) in sessions.iter().enumerate() {
                                    info!(
                                        "  {}. {} [{}] - 未读: {}",
                                        idx + 1,
                                        session
                                            .display_name
                                            .as_ref()
                                            .unwrap_or(&session.session_id),
                                        session.session_type,
                                        session.unread_count
                                    );
                                }
                                info!("");
                            } else {
                                warn!("获取会话列表失败");
                            }
                            continue;
                        } else if input == "/history" {
                            // 显示最近消息历史
                            use flare_im_core_sdk::api::traits::MessageApi;
                            if let Ok(messages) = client_for_status
                                .client
                                .get_messages(&session_id_for_input, 10, None)
                                .await
                            {
                                info!("");
                                info!("📜 最近消息历史 (共 {} 条):", messages.len());
                                for msg in messages.iter().rev() {
                                    let time_str = format_timestamp(msg.timestamp);
                                    let status_str = format_message_status(msg.status);
                                    let sender_name = if msg.sender_id == my_user_id_for_input {
                                        "我"
                                    } else {
                                        &msg.sender_id
                                    };

                                    if let Some(text_content) = &msg.content.text {
                                        info!(
                                            "  [{}] {} ({}): {}",
                                            time_str, sender_name, status_str, text_content.text
                                        );
                                    } else {
                                        info!(
                                            "  [{}] {} ({}): [{}]",
                                            time_str,
                                            sender_name,
                                            status_str,
                                            message_type_to_chinese(msg.message_type)
                                        );
                                    }
                                }
                                info!("");
                            } else {
                                warn!("获取消息历史失败");
                            }
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
                        // 发送消息（传递接收者ID）
                        let session_id_for_send = session_id.clone();
                        send_message(
                            &client.client,
                            &session_id_for_send,
                            &client.chat_with, // 使用存储的接收者ID
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
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde::{Deserialize, Serialize};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String, // user_id
        iss: String, // issuer
        exp: usize,  // expiration time
        iat: usize,  // issued at
        jti: String, // JWT ID
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
///
/// # 参数
/// - `server_url`: 服务器地址
/// - `user_id`: 当前登录用户ID
/// - `device_id`: 设备ID
/// - `token`: 认证Token
/// - `chat_with`: 聊天对象ID（用于设置receiver_id）
async fn create_client(
    server_url: &str,
    user_id: &str,
    device_id: &str,
    token: &str,
    chat_with: &str,
) -> Result<ClientInfo> {
    // 优化：预分配 Vec 容量
    let protocols = vec![TransportProtocol::WebSocket, TransportProtocol::QUIC];

    // 优化：预分配 HashMap 容量
    let mut protocol_urls = HashMap::with_capacity(2);
    protocol_urls.insert(TransportProtocol::WebSocket, server_url.to_string());
    let quic_url = server_url
        .replace("ws://", "quic://")
        .replace("/ws", "")
        .replace("60051", "60052");
    protocol_urls.insert(TransportProtocol::QUIC, quic_url);

    // 按用户ID生成不同的DB文件路径，避免不同用户数据混淆
    // 格式：flare-im-{user_id}.db
    // 注意：user_id 可能包含特殊字符，需要进行安全处理
    let safe_user_id = user_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let db_path = format!("flare-im-{}.db", safe_user_id);

    info!("📦 使用用户专属数据库: {}", db_path);

    let config = ClientConfig::builder()
        .server_url(server_url)
        .user_id(user_id)
        .device_id(device_id)
        .device_platform(flare_im_core_sdk::DevicePlatform::Desktop)
        .protocols(protocols)
        .protocol_urls(protocol_urls)
        .connect_timeout(30) // 增加超时时间
        .race_timeout(Duration::from_secs(30))
        .heartbeat_interval(30)
        .auto_reconnect(true)
        .max_reconnect_attempts(10)
        .token(token)
        .db_path(db_path) // 设置用户专属DB路径
        .build()?;

    let client = FlareIMClient::new(config).await?;

    Ok(ClientInfo {
        client,
        user_id: user_id.to_string(),
        chat_with: chat_with.to_string(),
    })
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
    let sessions = client
        .get_sessions(flare_im_core_sdk::SessionFilter::default())
        .await?;

    // 查找与目标用户的会话
    for session in &sessions {
        // 优先匹配标准会话ID
        if session.session_id == session_id {
            info!("✅ 找到现有会话: {}", session.session_id);

            // 确保单聊会话的 metadata 中包含 peer_id（用于消息推送）
            // 如果 metadata 中没有 peer_id 或 peer_id 不正确，更新会话设置 peer_id 为对方用户ID
            if session.session_type == "single" {
                let current_peer_id = session.metadata.get("peer_id");
                let needs_update =
                    current_peer_id.is_none() || current_peer_id != Some(&chat_with.to_string());

                if needs_update {
                    info!(
                        "   更新会话 metadata，设置 peer_id 为对方用户ID: {}",
                        chat_with
                    );
                    let mut metadata = session.metadata.clone();
                    metadata.insert("peer_id".to_string(), chat_with.to_string());

                    // 使用 storage 直接更新会话 metadata
                    let updates = flare_im_core_sdk::SessionUpdate::new().with_metadata(metadata);

                    if let Err(e) = client
                        .storage()
                        .update_session(&session.session_id, updates)
                        .await
                    {
                        warn!("⚠️  更新会话 metadata 失败: {}", e);
                    } else {
                        info!("✅ 会话 metadata 更新成功，peer_id = {}", chat_with);
                    }
                }
            }

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

    // 使用 API 层的 create_session 创建会话
    match client
        .create_session(
            Some(session_id.clone()),
            "single".to_string(),
            "chat".to_string(),
            Some(chat_with.to_string()), // 使用对方用户ID作为 display_name
            Some(vec![my_user_id.to_string(), chat_with.to_string()]), // 参与者列表
        )
        .await
    {
        Ok(created_session_id) => {
            info!("✅ 会话创建成功: {}", created_session_id);

            // 确保会话 metadata 中包含 peer_id（双重保险）
            if let Ok(Some(existing_session)) =
                client.storage().get_session(&created_session_id).await
            {
                let current_peer_id = existing_session.metadata.get("peer_id");
                if current_peer_id.is_none() || current_peer_id != Some(&chat_with.to_string()) {
                    info!("   补充设置 peer_id: {}", chat_with);
                    let mut metadata = existing_session.metadata.clone();
                    metadata.insert("peer_id".to_string(), chat_with.to_string());
                    let updates = flare_im_core_sdk::SessionUpdate::new().with_metadata(metadata);
                    if let Err(e) = client
                        .storage()
                        .update_session(&created_session_id, updates)
                        .await
                    {
                        warn!("⚠️  更新会话 metadata 失败: {}", e);
                    } else {
                        info!("✅ 会话 metadata 已更新，peer_id = {}", chat_with);
                    }
                }
            }

            Ok(created_session_id)
        }
        Err(e) => {
            // 如果服务端创建失败，记录警告但继续使用会话ID
            warn!("⚠️  服务端会话创建失败: {}", e);
            info!(
                "💡 提示: 将使用会话ID {} 等待对方创建会话或接收消息",
                session_id
            );
            Ok(session_id)
        }
    }
}

/// 处理接收到的消息
async fn handle_incoming_messages(client: &Arc<ClientInfo>, session_id: &str, my_user_id: &str) {
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
            Event::Message(MessageEvent::MessageReceived {
                message_id,
                session_id: msg_session_id,
            }) => {
                if msg_session_id == session_id {
                    // 等待一小段时间，确保消息已保存到本地存储
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    // 使用 API 层的 get_messages 获取消息（返回 MessageVO）
                    // 优化：使用更小的 limit，减少内存使用
                    if let Ok(messages) = client.client.get_messages(msg_session_id, 20, None).await
                    {
                        // 查找匹配的消息
                        if let Some(message) = messages.iter().find(|m| m.message_id == *message_id)
                        {
                            // 检查是否是来自聊天对象的消息（不是自己发送的）
                            if message.sender_id != my_user_id {
                                // 过滤掉 Typing 消息，不显示
                                if message.message_type == flare_proto::MessageType::Typing as i32 {
                                    debug!("收到 Typing 消息，跳过显示: message_id={}", message_id);
                                    continue;
                                }

                                // 检查消息是否被编辑过
                                let is_edited = !message.edit_history.is_empty();
                                let edit_version = if is_edited {
                                    message.edit_history.len() as i32
                                } else {
                                    0
                                };

                                // 对于编辑后的消息，使用 message_id + edit_version 作为唯一标识
                                let message_key = if is_edited {
                                    format!("{}:edit:{}", message_id, edit_version)
                                } else {
                                    message_id.clone()
                                };

                                // 检查是否已经处理过这条消息（或这个版本）
                                if processed_messages.contains(&message_key) {
                                    continue;
                                }

                                // 标记为已处理
                                processed_messages.insert(message_key);

                                // 显示接收到的消息（使用 MessageVO）
                                display_received_message(message, is_edited);

                                // 自动标记消息为已读（单聊场景）
                                use flare_im_core_sdk::api::traits::SessionApi;
                                if let Err(e) = client.client.mark_read(msg_session_id, None).await
                                {
                                    debug!("自动标记已读失败: {}", e);
                                }
                            }
                        } else {
                            debug!("消息尚未保存到本地存储或已过期: message_id={}", message_id);
                        }
                    } else {
                        debug!("获取消息失败: message_id={}", message_id);
                    }
                }
            }
            Event::Message(MessageEvent::MessageRecalled {
                message_id,
                session_id: msg_session_id,
            }) => {
                if msg_session_id == session_id {
                    info!("");
                    info!("🗑️  消息已被撤回 [消息ID: {}]", message_id);
                    info!("");
                }
            }
            Event::Message(MessageEvent::MessageStatusUpdated {
                message_id,
                session_id: msg_session_id,
                status,
            }) => {
                if msg_session_id == session_id {
                    // 检查是否是撤回状态
                    let recalled_status = flare_proto::MessageStatus::Recalled as i32;
                    if *status == recalled_status {
                        info!("");
                        info!("🗑️  消息状态更新：已撤回 [消息ID: {}]", message_id);
                        info!("");
                    } else {
                        // 其他状态更新（如已读、已送达等）可以在这里处理
                        debug!("消息状态更新: message_id={}, status={}", message_id, status);
                    }
                }
            }
            Event::Message(MessageEvent::MessageSent {
                message_id,
                session_id: msg_session_id,
            }) => {
                if msg_session_id == session_id {
                    debug!("✅ 消息发送成功: {}", message_id);
                }
            }
            Event::Message(MessageEvent::MessageStatusUpdated {
                message_id,
                session_id: msg_session_id,
                status,
            }) => {
                if msg_session_id == session_id {
                    let status_str = match *status {
                        x if x == flare_proto::MessageStatus::Sent as i32 => "已发送",
                        x if x == flare_proto::MessageStatus::Delivered as i32 => "已送达",
                        x if x == flare_proto::MessageStatus::Read as i32 => "已读",
                        _ => "未知状态",
                    };
                    debug!("📊 消息状态更新: {} -> {}", message_id, status_str);
                }
            }
            Event::Session(SessionEvent::SessionCreated {
                session_id: sess_id,
            }) => {
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

/// 显示接收到的消息（使用 MessageVO）
fn display_received_message(message: &MessageVO, is_edited: bool) {
    // 格式化时间戳
    let time_str = format_timestamp(message.timestamp);

    // 显示消息
    info!("");
    if is_edited {
        info!(
            "✏️  [{}] {} 发送了编辑后的消息:",
            time_str, message.sender_id
        );
    } else {
        info!("📨 [{}] {} 发送了新消息:", time_str, message.sender_id);
    }

    // 根据消息内容类型显示
    if let Some(text_content) = &message.content.text {
        let cleaned_text = clean_text(&text_content.text);
        if !cleaned_text.is_empty() {
            info!("   {}", cleaned_text);
        }

        // 显示 @提及
        if !text_content.mentions.is_empty() {
            let mentions: Vec<String> = text_content
                .mentions
                .iter()
                .filter_map(|m| m.user_id.clone())
                .collect();
            if !mentions.is_empty() {
                info!("   @提及: {}", mentions.join(", "));
            }
        }
    } else if let Some(image_content) = &message.content.image {
        info!(
            "   [图片] {}",
            image_content.description.as_ref().unwrap_or(&String::new())
        );
        if let Some(thumbnail) = &image_content.thumbnail {
            info!("   缩略图: {}x{}", thumbnail.width, thumbnail.height);
        }
    } else if let Some(video_content) = &message.content.video {
        info!(
            "   [视频] {}",
            video_content.description.as_ref().unwrap_or(&String::new())
        );
    } else if let Some(audio_content) = &message.content.audio {
        info!("   [语音] {}ms", audio_content.source.duration_ms);
    } else if let Some(file_content) = &message.content.file {
        info!(
            "   [文件] {} ({} bytes)",
            file_content.file_name, file_content.file_size
        );
    } else if let Some(location_content) = &message.content.location {
        info!(
            "   [位置] {}, {}",
            location_content.latitude, location_content.longitude
        );
        info!("   地址: {}", location_content.address);
    } else if let Some(card_content) = &message.content.card {
        info!(
            "   [名片] {} ({})",
            card_content.nickname, card_content.user_id
        );
    } else if let Some(quote_content) = &message.content.quote {
        info!("   [引用] {}", quote_content.quoted_text_preview);
    } else if let Some(notification_content) = &message.content.notification {
        info!(
            "   [通知] {}: {}",
            notification_content.title, notification_content.body
        );
    } else {
        info!("   [{}]", message_type_to_chinese(message.message_type));
    }

    // 显示消息状态
    let status_str = format_message_status(message.status);
    if message.status != flare_proto::MessageStatus::Created as i32 {
        info!("   状态: {}", status_str);
    }

    // 显示反应（如果有）
    if !message.reactions.is_empty() {
        let reactions: Vec<String> = message
            .reactions
            .iter()
            .map(|r| format!("{} ({})", r.emoji, r.count))
            .collect();
        info!("   反应: {}", reactions.join(", "));
    }

    info!("");
}

/// 清理文本内容（移除控制字符）
fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| {
            if c.is_whitespace() {
                true
            } else {
                !c.is_control()
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// 格式化时间戳为可读字符串
fn format_timestamp(timestamp_ms: i64) -> String {
    use chrono::{DateTime, Local, TimeZone};
    if timestamp_ms > 0 {
        if let Some(dt) = Local.timestamp_millis_opt(timestamp_ms).single() {
            dt.format("%H:%M:%S").to_string()
        } else {
            "无效时间".to_string()
        }
    } else {
        "未知时间".to_string()
    }
}

/// 格式化消息状态
fn format_message_status(status: i32) -> &'static str {
    match status {
        x if x == flare_proto::MessageStatus::Created as i32 => "已创建",
        x if x == flare_proto::MessageStatus::Sent as i32 => "已发送",
        x if x == flare_proto::MessageStatus::Delivered as i32 => "已送达",
        x if x == flare_proto::MessageStatus::Read as i32 => "已读",
        x if x == flare_proto::MessageStatus::Failed as i32 => "发送失败",
        x if x == flare_proto::MessageStatus::Recalled as i32 => "已撤回",
        _ => "未知状态",
    }
}

/// 发送消息（使用 API 层）
///
/// # 参数
/// - `client`: 客户端实例
/// - `session_id`: 会话ID
/// - `chat_with`: 接收者ID（用于设置receiver_id）
/// - `text`: 消息内容
async fn send_message(client: &FlareIMClient, session_id: &str, chat_with: &str, text: &str) {
    use flare_im_core_sdk::api::traits::MessageApi;
    use flare_im_core_sdk::api::traits::SessionApi;
    use flare_im_core_sdk::domain::MessageBuilder;

    // 先异步获取 user_id（避免在同步方法中使用 blocking_read）
    let user_id = match client.user_id().await {
        Ok(uid) => uid,
        Err(e) => {
            error!("❌ 获取用户ID失败: {}", e);
            return;
        }
    };

    // 使用 MessageBuilder 构建消息（避免使用同步的 create_text_message）
    let proto_message = MessageBuilder::new()
        .session_id(session_id.to_string())
        .sender_id(user_id)
        .text(text.to_string())
        .metadata("business_type".to_string(), "chat".to_string())
        .build();

    // 将 ProtoMessage 转换为 DomainMessage
    use flare_im_core_sdk::domain::message::Message as DomainMessage;
    let message = match DomainMessage::from_proto(proto_message) {
        Ok(msg) => msg,
        Err(e) => {
            error!("❌ 创建消息失败: {}", e);
            return;
        }
    };

    // 使用 API 层的 send_message 发送消息
    // 单聊需要指定 receiver_id
    match client
        .send_message(message, Some(chat_with.to_string()), None)
        .await
    {
        Ok(message_id) => {
            debug!("✅ 消息发送成功: {}", message_id);

            // 自动标记消息为已读（单聊场景，发送后自动标记）
            if let Err(e) = client.mark_read(session_id, None).await {
                debug!("自动标记已读失败: {}", e);
            }
        }
        Err(e) => {
            error!("❌ 消息发送失败: {}", e);
            info!("💡 提示: 请检查连接状态和会话ID是否正确");
        }
    }
}
