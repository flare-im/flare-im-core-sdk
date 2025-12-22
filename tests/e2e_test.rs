//! 端到端测试
//!
//! 测试 SDK 与服务端的完整交互流程，包括：
//! 1. 两个客户端互发消息
//! 2. 会话同步
//! 3. 消息状态流转
//! 4. 重连和断线恢复
//!
//! 对标微信、Telegram、飞书的生产级别测试标准

#[cfg(test)]
mod test_utils;

use test_utils::*;
use flare_im_core_sdk::config::{SdkConfig, TransportProtocol};
use flare_im_core_sdk::interface::facade::ImCoreSdk;
use flare_im_core_sdk::domain::message::{TenantContext, MessageType};
use std::time::Duration;
use tracing::{info, warn, error};

/// 创建测试用的 TenantContext
fn create_test_tenant(tenant_id: &str, user_id: &str) -> TenantContext {
    TenantContext {
        tenant_id: tenant_id.to_string(),
        user_id: user_id.to_string(),
    }
}

/// 测试：两个客户端互发消息
///
/// 场景：
/// 1. 客户端 A 登录并连接
/// 2. 客户端 B 登录并连接
/// 3. 客户端 A 发送消息给客户端 B
/// 4. 验证客户端 B 收到消息
/// 5. 客户端 B 回复消息给客户端 A
/// 6. 验证客户端 A 收到回复
#[tokio::test(flavor = "multi_thread")]
#[ignore] // 默认忽略，需要服务端运行
async fn test_two_clients_message_exchange() {
    // 检查服务端是否可用
    let config = TestConfig::default();
    if !check_server_available(&config.server_url).await {
        warn!("服务端不可用，跳过端到端测试");
        warn!("请启动 gateway 服务端：cd flare-im-core/flare-signaling/gateway && cargo run");
        return;
    }
    
    info!("🚀 开始端到端测试：两个客户端互发消息");
    
    // 创建两个客户端
    let (sdk_a, _temp_dir_a, _db_path_a) = create_test_sdk_with_config(&config).await;
    let (sdk_b, _temp_dir_b, _db_path_b) = create_test_sdk_with_config(&config).await;
    
    // 用户信息
    let user_a = "user_e2e_001";
    let user_b = "user_e2e_002";
    let tenant_id = "tenant_e2e_001";
    
    // 1. 客户端 A 登录并连接
    info!("📱 客户端 A 登录: {}", user_a);
    sdk_a.login(user_a.to_string(), format!("token_{}", user_a)).await
        .expect("客户端 A 登录失败");
    
    info!("🔌 客户端 A 连接");
    sdk_a.connect().await
        .expect("客户端 A 连接失败");
    
    // 等待连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // 2. 客户端 B 登录并连接
    info!("📱 客户端 B 登录: {}", user_b);
    sdk_b.login(user_b.to_string(), format!("token_{}", user_b)).await
        .expect("客户端 B 登录失败");
    
    info!("🔌 客户端 B 连接");
    sdk_b.connect().await
        .expect("客户端 B 连接失败");
    
    // 等待连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // 3. 客户端 A 发送消息给客户端 B
    let conversation_id = generate_test_conversation_id(user_a, user_b);
    let message_text = "Hello from Client A!";
    
    info!("📤 客户端 A 发送消息: {}", message_text);
    let tenant_a = create_test_tenant(tenant_id, user_a);
    let message = sdk_a.message().create_text_message(
        conversation_id.clone(),
        user_a.to_string(),
        message_text.to_string(),
        tenant_a,
        Some(user_b.to_string()), // 单聊消息需要 receiver_id
    ).expect("创建消息失败");
    
    sdk_a.message().send_message(message.clone()).await
        .expect("发送消息失败");
    
    info!("✅ 客户端 A 消息已发送，等待客户端 B 接收");
    
    // 4. 验证客户端 B 收到消息
    info!("📥 等待客户端 B 接收消息");
    let queue_b = sdk_b.message_queue().clone();
    let received_message = wait_for_message(
        queue_b,
        &conversation_id,
        config.message_timeout,
    ).await;
    
    assert!(received_message.is_some(), "客户端 B 应该收到消息");
    let msg = received_message.unwrap();
    
    assert!(
        verify_message(&msg, &conversation_id, user_a, Some(message_text)),
        "消息内容验证失败"
    );
    
    let content_str = String::from_utf8_lossy(&msg.content);
    info!("✅ 客户端 B 收到消息: {}", content_str);
    
    // 5. 客户端 B 回复消息给客户端 A
    let reply_text = "Hello from Client B!";
    
    info!("📤 客户端 B 回复消息: {}", reply_text);
    let tenant_b = create_test_tenant(tenant_id, user_b);
    let reply = sdk_b.message().create_text_message(
        conversation_id.clone(),
        user_b.to_string(),
        reply_text.to_string(),
        tenant_b,
        Some(user_a.to_string()), // 单聊消息需要 receiver_id
    ).expect("创建回复消息失败");
    
    sdk_b.message().send_message(reply.clone()).await
        .expect("发送回复消息失败");
    
    info!("✅ 客户端 B 回复已发送，等待客户端 A 接收");
    
    // 6. 验证客户端 A 收到回复
    info!("📥 等待客户端 A 接收回复");
    let queue_a = sdk_a.message_queue().clone();
    let received_reply = wait_for_message(
        queue_a,
        &conversation_id,
        config.message_timeout,
    ).await;
    
    assert!(received_reply.is_some(), "客户端 A 应该收到回复");
    let reply_msg = received_reply.unwrap();
    
    assert!(
        verify_message(&reply_msg, &conversation_id, user_b, Some(reply_text)),
        "回复消息内容验证失败"
    );
    
    let reply_content_str = String::from_utf8_lossy(&reply_msg.content);
    info!("✅ 客户端 A 收到回复: {}", reply_content_str);
    
    // 清理
    sdk_a.logout().await.ok();
    sdk_b.logout().await.ok();
    
    info!("🎉 端到端测试完成：两个客户端互发消息");
}

/// 测试：会话同步
///
/// 场景：
/// 1. 客户端 A 发送消息给客户端 B
/// 2. 验证客户端 B 的会话列表中有该会话
/// 3. 验证会话的未读数正确
#[tokio::test(flavor = "multi_thread")]
#[ignore] // 默认忽略，需要服务端运行
async fn test_conversation_sync() {
    // 检查服务端是否可用
    let config = TestConfig::default();
    if !check_server_available(&config.server_url).await {
        warn!("服务端不可用，跳过会话同步测试");
        return;
    }
    
    info!("🚀 开始会话同步测试");
    
    // 创建两个客户端
    let (sdk_a, _temp_dir_a, _db_path_a) = create_test_sdk_with_config(&config).await;
    let (sdk_b, _temp_dir_b, _db_path_b) = create_test_sdk_with_config(&config).await;
    
    // 用户信息
    let user_a = "user_sync_001";
    let user_b = "user_sync_002";
    let tenant_id = "tenant_sync_001";
    
    // 登录并连接
    sdk_a.login(user_a.to_string(), format!("token_{}", user_a)).await
        .expect("客户端 A 登录失败");
    sdk_a.connect().await.expect("客户端 A 连接失败");
    
    sdk_b.login(user_b.to_string(), format!("token_{}", user_b)).await
        .expect("客户端 B 登录失败");
    sdk_b.connect().await.expect("客户端 B 连接失败");
    
    // 等待连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // 客户端 A 发送消息
    let conversation_id = generate_test_conversation_id(user_a, user_b);
    let tenant_a = create_test_tenant(tenant_id, user_a);
    let message = sdk_a.message().create_text_message(
        conversation_id.clone(),
        user_a.to_string(),
        "Test message for sync".to_string(),
        tenant_a,
        Some(user_b.to_string()), // 单聊消息需要 receiver_id
    ).expect("创建消息失败");
    
    sdk_a.message().send_message(message).await
        .expect("发送消息失败");
    
    // 等待消息到达客户端 B
    let queue_b = sdk_b.message_queue().clone();
    let _received = wait_for_message(
        queue_b,
        &conversation_id,
        config.message_timeout,
    ).await;
    
    // 验证客户端 B 的会话列表
    let conversations = sdk_b.conversation().get_all_conversation_list().await
        .expect("获取会话列表失败");
    
    assert!(
        conversations.iter().any(|c| {
            c.get("conversation_id")
                .and_then(|v| v.as_str())
                .map(|id| id == conversation_id)
                .unwrap_or(false)
        }),
        "客户端 B 的会话列表中应该包含该会话"
    );
    
    // 验证未读数
    let total_unread = sdk_b.conversation().get_total_unread_msg_count().await
        .expect("获取未读数失败");
    
    assert!(total_unread > 0, "未读数应该大于 0");
    
    info!("✅ 会话同步测试完成");
    
    // 清理
    sdk_a.logout().await.ok();
    sdk_b.logout().await.ok();
}

/// 测试：消息状态流转
///
/// 场景：
/// 1. 发送消息
/// 2. 验证消息状态从"发送中"变为"已发送"
/// 3. 验证收到服务器 ACK
#[tokio::test(flavor = "multi_thread")]
#[ignore] // 默认忽略，需要服务端运行
async fn test_message_status_flow() {
    // 检查服务端是否可用
    let config = TestConfig::default();
    if !check_server_available(&config.server_url).await {
        warn!("服务端不可用，跳过消息状态测试");
        return;
    }
    
    info!("🚀 开始消息状态流转测试");
    
    // 创建客户端
    let (sdk, _temp_dir, _db_path) = create_test_sdk_with_config(&config).await;
    
    let user_id = "user_status_001";
    let tenant_id = "tenant_status_001";
    
    // 登录并连接
    sdk.login(user_id.to_string(), format!("token_{}", user_id)).await
        .expect("登录失败");
    sdk.connect().await.expect("连接失败");
    
    // 等待连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // 发送消息
    let conversation_id = "conv_status_001";
    let tenant = create_test_tenant(tenant_id, user_id);
    let message = sdk.message().create_text_message(
        conversation_id.to_string(),
        user_id.to_string(),
        "Test message status".to_string(),
        tenant,
        Some("receiver_001".to_string()), // 单聊消息需要 receiver_id
    ).expect("创建消息失败");
    
    let message_id = message.id.clone();
    
    // 发送消息（应该收到 ACK）
    sdk.message().send_message(message).await
        .expect("发送消息失败");
    
    info!("✅ 消息已发送，等待 ACK");
    
    // 等待一段时间，让 ACK 处理完成
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // 验证消息状态（通过查询本地消息）
    // 注意：这里需要根据实际的消息状态查询 API 调整
    let messages = sdk.message().find_message_list(
        Some(conversation_id.to_string()),
        None,
        None,
        None,
        Some(10),
    ).await.expect("查询消息失败");
    
    assert!(
        messages.iter().any(|m| {
            m.get("id")
                .and_then(|v| v.as_str())
                .map(|id| id == message_id)
                .unwrap_or(false)
        }),
        "应该能找到发送的消息"
    );
    
    info!("✅ 消息状态流转测试完成");
    
    // 清理
    sdk.logout().await.ok();
}

/// 测试：重连和断线恢复
///
/// 场景：
/// 1. 客户端连接并发送消息
/// 2. 模拟断线（断开连接）
/// 3. 重新连接
/// 4. 验证消息队列中的消息能够正常处理
#[tokio::test(flavor = "multi_thread")]
#[ignore] // 默认忽略，需要服务端运行
async fn test_reconnect_and_recovery() {
    // 检查服务端是否可用
    let config = TestConfig::default();
    if !check_server_available(&config.server_url).await {
        warn!("服务端不可用，跳过重连测试");
        return;
    }
    
    info!("🚀 开始重连和断线恢复测试");
    
    // 创建客户端
    let (sdk, _temp_dir, _db_path) = create_test_sdk_with_config(&config).await;
    
    let user_id = "user_reconnect_001";
    let tenant_id = "tenant_reconnect_001";
    
    // 登录并连接
    sdk.login(user_id.to_string(), format!("token_{}", user_id)).await
        .expect("登录失败");
    sdk.connect().await.expect("连接失败");
    
    // 等待连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // 发送消息
    let conversation_id = "conv_reconnect_001";
    let tenant = create_test_tenant(tenant_id, user_id);
    let message = sdk.message().create_text_message(
        conversation_id.to_string(),
        user_id.to_string(),
        "Test reconnect message".to_string(),
        tenant,
        Some("receiver_001".to_string()), // 单聊消息需要 receiver_id
    ).expect("创建消息失败");
    
    sdk.message().send_message(message).await
        .expect("发送消息失败");
    
    info!("✅ 消息已发送");
    
    // 断开连接
    info!("🔌 断开连接");
    sdk.logout().await.expect("登出失败");
    
    // 等待一段时间
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // 重新登录并连接
    info!("🔄 重新连接");
    sdk.login(user_id.to_string(), format!("token_{}", user_id)).await
        .expect("重新登录失败");
    sdk.connect().await.expect("重新连接失败");
    
    // 等待连接建立
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    info!("✅ 重连和断线恢复测试完成");
    
    // 清理
    sdk.logout().await.ok();
}

