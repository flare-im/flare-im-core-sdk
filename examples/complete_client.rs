//! Flare IM SDK 完整功能示例
//!
//! 演示 SDK 的所有核心功能：
//! - 连接和认证
//! - 消息发送和接收（支持优先级队列和批处理）
//! - 会话管理
//! - 消息同步
//! - 事件监听
//! - 错误处理和恢复
//! - 生命周期管理
//!
//! ## 运行方式
//!
//! ```bash
//! # 设置日志级别
//! RUST_LOG=info cargo run --example complete_client
//!
//! # 指定用户ID和服务器地址
//! RUST_LOG=info USER_ID=user123 SERVER_URL=ws://localhost:60051 cargo run --example complete_client
//! ```

use flare_im_core_sdk::{
    FlareIMClient, ClientConfig,
    Event, ConnectionEvent, MessageEvent, SessionEvent, SyncEvent,
    SendOptions, MessagePriority, SDKError,
};
use flare_core::common::config_types::TransportProtocol;
use flare_core::common::protocol::Reliability;
use std::collections::HashMap;
use tracing::{info, error, warn, debug};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("🚀 Flare IM SDK 完整功能示例");
    info!("==========================================");

    // ============================================================
    // 1. 配置客户端
    // ============================================================
    let server_url = std::env::var("SERVER_URL")
        .unwrap_or_else(|_| "ws://localhost:60051/ws".to_string());
    
    let user_id = std::env::var("USER_ID")
        .unwrap_or_else(|_| format!("user-{}", std::process::id()));
    
    let token = format!("token-{}", user_id); // 简单的 token 生成

    info!("📋 配置信息:");
    info!("   服务器地址: {}", server_url);
    info!("   用户ID: {}", user_id);
    info!("   设备ID: device-{}", std::process::id());

    // 协议配置（协议竞速）
    let protocols = vec![
        TransportProtocol::WebSocket,
        TransportProtocol::QUIC,
    ];

    let mut protocol_urls = HashMap::new();
    if let Some(port) = server_url.split(':').nth(2).and_then(|p| p.split('/').next()) {
        let host = server_url.split("://").nth(1)
            .and_then(|s| s.split(':').next())
            .unwrap_or("localhost");
        let ws_url = format!("ws://{}:{}", host, port);
        let quic_url = format!("quic://{}:{}", host, port.replace("60051", "60052"));
        protocol_urls.insert(TransportProtocol::WebSocket, ws_url);
        protocol_urls.insert(TransportProtocol::QUIC, quic_url);
    } else {
        protocol_urls.insert(TransportProtocol::WebSocket, "ws://localhost:60051/ws".to_string());
        protocol_urls.insert(TransportProtocol::QUIC, "quic://localhost:60052".to_string());
    }

    let config = ClientConfig::builder()
        .server_url(server_url.clone())
        .user_id(&user_id)
        .device_id(&format!("device-{}", std::process::id()))
        .device_platform(flare_im_core_sdk::DevicePlatform::Desktop)
        .protocols(protocols)
        .protocol_urls(protocol_urls)
        .connect_timeout(15)
        .race_timeout(std::time::Duration::from_secs(15))
        .heartbeat_interval(30)
        .auto_reconnect(true)
        .max_reconnect_attempts(10)
        .token(&token)
        .build()?;

    // ============================================================
    // 2. 创建客户端
    // ============================================================
    info!("📦 创建客户端实例...");
    let client = FlareIMClient::new(config).await?;
    info!("✅ 客户端创建成功");

    // ============================================================
    // 3. 订阅事件
    // ============================================================
    info!("📡 订阅事件...");
    let event_bus = client.event_bus();
    let mut event_rx = event_bus.subscribe();

    // 启动事件处理任务
    let event_handler = tokio::spawn(async move {
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
                Event::Connection(ConnectionEvent::Error(err)) => {
                    error!("❌ 连接错误: {}", err);
                }
                Event::Message(MessageEvent::MessageReceived { message_id, session_id }) => {
                    info!("📨 收到消息: message_id={}, session_id={}", message_id, session_id);
                }
                Event::Message(MessageEvent::MessageSent { message_id, session_id }) => {
                    info!("📤 消息已发送: message_id={}, session_id={}", message_id, session_id);
                }
                Event::Session(SessionEvent::SessionCreated { session_id }) => {
                    info!("💬 会话已创建: {}", session_id);
                }
                Event::Session(SessionEvent::UnreadCountChanged { session_id, count }) => {
                    debug!("📊 未读数变化: session_id={}, count={}", session_id, count);
                }
                Event::Sync(SyncEvent::SyncCompleted { sessions, messages, .. }) => {
                    info!("✅ 同步完成: sessions={}, messages={}", sessions, messages);
                }
                Event::Sync(SyncEvent::SyncFailed { error, .. }) => {
                    error!("❌ 同步失败: {}", error);
                }
                _ => {
                    debug!("📢 其他事件: {:?}", event);
                }
            }
        }
    });

    // ============================================================
    // 4. 登录
    // ============================================================
    info!("🔐 开始登录...");
    let login_result = client.login(&user_id, &token).await?;
    info!("✅ 登录成功: {:?}", login_result);

    // 等待连接稳定
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // ============================================================
    // 5. 同步会话和消息
    // ============================================================
    info!("🔄 开始同步会话...");
    match client.sync_sessions(None).await {
        Ok(result) => {
            info!("✅ 会话同步成功: 会话数={}", result.sessions.len());
            for session in &result.sessions {
                info!("   - 会话: {} (未读数: {})", session.session_id, session.unread_count);
            }
        }
        Err(e) => {
            warn!("⚠️  会话同步失败: {}", e);
        }
    }

    // ============================================================
    // 6. 获取会话列表
    // ============================================================
    info!("📋 获取会话列表...");
    match client.get_sessions(flare_im_core_sdk::SessionFilter::default()).await {
        Ok(sessions) => {
            info!("✅ 获取到 {} 个会话", sessions.len());
            if sessions.is_empty() {
                info!("   提示: 当前没有会话，可以创建新会话或等待其他用户发送消息");
            } else {
                for session in &sessions {
                    info!("   - {}: 未读数={}, 最后消息时间={:?}", 
                        session.session_id, 
                        session.unread_count,
                        session.last_message_time
                    );
                }
            }
        }
        Err(e) => {
            error!("❌ 获取会话列表失败: {}", e);
        }
    }

    // ============================================================
    // 7. 发送测试消息（如果有会话）
    // ============================================================
    if let Ok(sessions) = client.get_sessions(flare_im_core_sdk::SessionFilter::default()).await {
        if let Some(first_session) = sessions.first() {
            let session_id = &first_session.session_id;
            info!("📤 发送测试消息到会话: {}", session_id);
            
            // 7.1 发送普通消息
            let content1 = flare_proto::MessageContent {
                content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
                    flare_proto::TextContent {
                        text: format!("Hello from SDK! Time: {:?}", std::time::SystemTime::now()),
                        mentions: vec![],
                    }
                )),
            };

            match client.send_message(session_id, content1).await {
                Ok(message_id) => {
                    info!("✅ 普通消息发送成功: message_id={}", message_id);
                }
                Err(e) => {
                    error!("❌ 普通消息发送失败: {}", e);
                }
            }

            // 7.2 发送高优先级消息（使用新的 SendOptions）
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            
            let content2 = flare_proto::MessageContent {
                content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
                    flare_proto::TextContent {
                        text: "这是一条高优先级消息！".to_string(),
                        mentions: vec![],
                    }
                )),
            };

            // 使用消息服务的 send_message_with_options 方法发送高优先级消息
            let message_service = client.message_service();
            let send_options = SendOptions {
                reliability: Reliability::AtLeastOnce,
                priority: Some(8), // 高优先级（对应 MessagePriority::High）
            };

            match message_service.send_message_with_options(session_id, content2, send_options).await {
                Ok(message_id) => {
                    info!("✅ 高优先级消息发送成功: message_id={}", message_id);
                }
                Err(e) => {
                    error!("❌ 高优先级消息发送失败: {}", e);
                }
            }

            // 7.3 演示错误处理
            info!("🔍 演示错误处理...");
            let test_content = flare_proto::MessageContent {
                content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
                    flare_proto::TextContent {
                        text: "测试错误处理".to_string(),
                        mentions: vec![],
                    }
                )),
            };
            match client.send_message("invalid_session_id", test_content).await {
                Ok(_) => {
                    warn!("⚠️  意外成功：应该失败的消息却成功了");
                }
                Err(e) => {
                    // 使用新的错误处理体系
                    // 将 anyhow::Error 转换为 SDKError
                    let sdk_error: SDKError = e.into();
                    if let Some(error_code) = sdk_error.code() {
                        info!("✅ 错误处理正常: 错误码={:?}, 消息={}", error_code, sdk_error.message());
                    } else {
                        info!("✅ 错误处理正常: 消息={}", sdk_error.message());
                    }
                }
            }
        } else {
            info!("💡 提示: 当前没有会话，无法发送测试消息");
            info!("   可以启动另一个客户端实例，使用相同的用户ID创建会话");
        }
    }

    // ============================================================
    // 8. 演示生命周期管理
    // ============================================================
    info!("🔧 检查客户端健康状态...");
    let connection_state = client.connection_state().await;
    info!("   连接状态: {:?}", connection_state);
    
    // 获取用户ID
    match client.user_id().await {
        Ok(uid) => {
            info!("   当前用户ID: {}", uid);
        }
        Err(e) => {
            warn!("   获取用户ID失败: {}", e);
        }
    }

    // ============================================================
    // 9. 保持运行，监听消息
    // ============================================================
    info!("");
    info!("==========================================");
    info!("✅ 客户端运行中，等待消息...");
    info!("   功能特性:");
    info!("   - ✅ 统一错误处理体系");
    info!("   - ✅ 消息优先级队列和批处理");
    info!("   - ✅ 自动重试和错误恢复");
    info!("   - ✅ 生命周期管理");
    info!("   按 Ctrl+C 退出");
    info!("==========================================");
    info!("");

    // 等待用户中断
    tokio::signal::ctrl_c().await?;
    info!("");
    info!("👋 正在关闭客户端...");

    // 取消事件处理任务
    event_handler.abort();

    // 登出（优雅关闭）
    info!("🔒 正在登出...");
    if let Err(e) = client.logout().await {
        warn!("⚠️  登出失败: {}", e);
    } else {
        info!("✅ 登出成功");
    }

    info!("✅ 客户端已关闭");
    Ok(())
}

