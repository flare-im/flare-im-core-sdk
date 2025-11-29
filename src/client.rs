//! Flare IM Client - SDK 主入口
//!
//! 整合所有模块，提供统一的 API

use crate::config::ClientConfig;
use crate::connection::ConnectionManager;
use crate::event::{Event, EventBus, ConnectionEvent, MessageEvent};
use crate::model::{
    Message,
    SessionSummary,
};
use crate::service::{
    MessageService, SessionService, SyncService,
};
#[cfg(feature = "extensions")]
use crate::extension::ExtensionInfoManager as ExtensionManager;
use crate::storage::{StorageBackend, SessionFilter};
use anyhow::{Context, Result};
use std::sync::Arc;
 
use tracing::{info, warn};
use tokio::sync::RwLock;
#[cfg(target_arch = "wasm32")]
use tokio::task::spawn_local as tokio_spawn;
#[cfg(not(target_arch = "wasm32"))]
use tokio::spawn as tokio_spawn;

/// Flare IM 客户端主入口
/// 
/// 整合所有模块，提供统一的 API
pub struct FlareIMClient {
    /// 连接管理器
    connection: Arc<ConnectionManager>,
    
    /// 消息服务
    message_service: Arc<MessageService>,
    
    /// 会话服务
    session_service: Arc<SessionService>,
    
    /// 同步服务
    sync_service: Arc<SyncService>,
    
    /// 本地存储
    storage: Arc<dyn StorageBackend>,
    
    /// 事件总线
    event_bus: Arc<EventBus>,
    
    /// 配置
    config: Arc<tokio::sync::RwLock<ClientConfig>>,
    
    /// 当前用户 ID
    user_id: Arc<RwLock<String>>,
    
    /// 消息帧处理器
    message_frame_handler: Arc<crate::handler::MessageFrameHandler>,
    
    /// 消息观察者注册表
    observer_registry: Arc<crate::observer::MessageObserverRegistry>,
    
    /// 扩展管理器（用于填充扩展信息）
    /// 如果启用了 extensions feature，则必需；否则使用空实现
    #[cfg(feature = "extensions")]
    extension_manager: Arc<ExtensionManager>,
}

/// 登录结果
#[derive(Debug, Clone)]
pub struct LoginResult {
    /// 用户 ID
    pub user_id: String,
    
    /// 会话 ID
    pub session_id: String,
}

impl FlareIMClient {
    /// 创建客户端实例（预初始化模式）
    /// 
    /// 此方法创建客户端但不连接，适用于应用启动时预初始化
    /// 登录时只需要调用 `login()` 即可，无需重新创建客户端
    /// 
    /// # 参数
    /// - `config`: 客户端配置（可以暂时不设置 user_id 和 token，登录时再设置）
    /// 
    /// # 返回
    /// - `Result<FlareIMClient>`: 客户端实例（未连接状态）
    /// 
    /// # 示例
    /// ```rust,no_run
    /// // 应用启动时预初始化
    /// let config = ClientConfig::builder()
    ///     .server_url("wss://im.example.com")
    ///     .device_id("device_123")
    ///     .build()?;
    /// let client = FlareIMClient::new(config).await?;
    /// 
    /// // 登录时直接使用
    /// client.login("user_123", "token").await?;
    /// ```
    pub async fn new(config: ClientConfig) -> Result<Self> {
        // 1. 创建事件总线
        let event_bus = Arc::new(EventBus::new());
        
        // 2. 创建存储（根据平台自动选择）
        let storage: Arc<dyn StorageBackend> = {
            use crate::platform::{get_platform, Platform};
            let platform = get_platform();
            
            match platform {
                Platform::Web => {
                    #[cfg(target_arch = "wasm32")]
                    {
                        use crate::storage::indexeddb::IndexedDBStorage;
                        Arc::new(IndexedDBStorage::new("flare-im").await
                            .context("Failed to create IndexedDB storage")?)
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        anyhow::bail!("Web platform requires wasm32 target");
                    }
                }
                _ => {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        use crate::storage::sqlite::SqliteStorage;
                        Arc::new(SqliteStorage::new(":memory:").await
                            .context("Failed to create SQLite storage")?)
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        anyhow::bail!("Non-web platforms require non-wasm32 target");
                    }
                }
            }
        };
        
        
        // 4. 创建共享的 config（用于 ConnectionManager 和 FlareIMClient）
        let shared_config = Arc::new(tokio::sync::RwLock::new(config));
        
        // 5. 创建共享 user_id（登录后更新）
        let user_id_shared = Arc::new(RwLock::new(String::new()));

        // 6. 创建同步服务（登录后会更新 user_id）
        // 注意：需要先创建 connection，但 connection 需要 message_listener
        // 所以先创建临时的 connection，稍后设置 message_listener
        let connection = Arc::new(ConnectionManager::new(
            Arc::clone(&shared_config),
            Arc::clone(&event_bus),
        ));
        
        let sync_service = Arc::new(SyncService::new(
            Arc::clone(&connection),
            Arc::clone(&storage),
            Arc::clone(&event_bus),
            Arc::clone(&user_id_shared),
        ).with_config(crate::service::sync::SyncConfig { request_timeout: 20, ..Default::default() }));

        // 7. 创建扩展管理器（如果启用了 extensions feature）
        #[cfg(feature = "extensions")]
        let extension_manager = Arc::new(ExtensionManager::new());
        #[cfg(feature = "extensions")]
        let extension_manager_opt: Option<Arc<ExtensionManager>> = Some(Arc::clone(&extension_manager));
        #[cfg(not(feature = "extensions"))]
        let extension_manager_opt: Option<()> = None;

        // 8. 创建会话服务（先创建，稍后设置扩展管理器）
        let session_service = {
            let mut service = SessionService::new(
                Arc::clone(&connection),
                Arc::clone(&storage),
                Arc::clone(&sync_service),
                Arc::clone(&event_bus),
                Arc::clone(&user_id_shared),
            );
            #[cfg(feature = "extensions")]
            {
                if let Some(ref ext_mgr) = extension_manager_opt {
                    service = service.with_extension_manager(Arc::clone(ext_mgr));
                }
            }
            Arc::new(service)
        };

        // 9. 创建消息服务（默认启用所有优化功能）
        let message_service = {
            let mut service = MessageService::new(
                Arc::clone(&connection),
                Arc::clone(&storage),
                Arc::clone(&event_bus),
                Arc::clone(&user_id_shared),
            ).with_session_service(Arc::clone(&session_service));
            #[cfg(feature = "extensions")]
            {
                if let Some(ref ext_mgr) = extension_manager_opt {
                    service = service.with_extension_manager(Arc::clone(ext_mgr));
                }
            }
            Arc::new(service)
        };

        // 10. 创建消息观察者注册表
        let observer_registry = Arc::new(crate::observer::MessageObserverRegistry::new());
        
        // 11. 创建消息帧处理器
        let message_frame_handler = Arc::new(
            crate::handler::MessageFrameHandler::new(
                Arc::clone(&message_service),
                Arc::clone(&event_bus),
            )
            .with_connection_manager(Arc::clone(&connection))
        );
        
        // 12. 创建 SDK 消息监听器（用于 FlareClientBuilder）
        let message_listener = Arc::new(crate::connection::message_listener::SDKMessageListener::new(
            Arc::clone(&message_frame_handler),
            Arc::clone(&sync_service),
            Arc::clone(&event_bus),
        ));
        
        // 13. 设置消息监听器到连接管理器（必须在连接前设置）
        connection.set_message_listener(message_listener).await;

        // 12. 启动消息接收监听器
        let handler_clone = Arc::clone(&message_frame_handler);
        let sync_service_clone = Arc::clone(&sync_service);
        let mut event_rx = event_bus.subscribe();
        tokio_spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                if let Event::Connection(ConnectionEvent::FrameReceived(frame)) = event {
                    // 检查是否是自定义命令（同步响应）或包含 request_id 的响应帧（可能是系统错误）
                    let is_custom_command = frame.command.as_ref()
                        .and_then(|cmd| cmd.r#type.as_ref())
                        .map(|t| matches!(t, flare_core::common::protocol::flare::core::commands::command::Type::Custom(_)))
                        .unwrap_or(false);
                    let has_request_id_meta = frame.metadata.get("request_id").is_some();
                    let cmd_name = frame.command.as_ref()
                        .and_then(|cmd| cmd.r#type.as_ref())
                        .map(|t| match t {
                            flare_core::common::protocol::flare::core::commands::command::Type::Message(_) => "Message",
                            flare_core::common::protocol::flare::core::commands::command::Type::System(_) => "System",
                            flare_core::common::protocol::flare::core::commands::command::Type::Custom(custom) => custom.name.as_str(),
                            flare_core::common::protocol::flare::core::commands::command::Type::Notification(_) => "Notification",
                        })
                        .unwrap_or("<none>");
                    let request_id = frame.metadata
                        .get("request_id")
                        .and_then(|v| String::from_utf8(v.clone()).ok())
                        .unwrap_or_else(|| frame.message_id.clone());
                    tracing::debug!(msg_id = %frame.message_id, %request_id, cmd = %cmd_name, "Routing incoming frame");
                    
                    if is_custom_command || has_request_id_meta {
                        // 自定义命令或带有 request_id 的系统响应，交由 SyncService 完成请求
                        if let Err(e) = sync_service_clone.handle_response(frame).await {
                            tracing::error!(error = %e, "Failed to handle sync response");
                        }
                    } else {
                        // 其他命令由 MessageFrameHandler 处理
                        if let Err(e) = handler_clone.handle_frame(frame).await {
                            tracing::error!(error = %e, "Failed to handle frame");
                        }
                    }
                }
            }
        });

        let session_service_clone = Arc::clone(&session_service);
        let storage_clone: Arc<dyn StorageBackend> = Arc::clone(&storage);
        let user_id_clone = Arc::clone(&user_id_shared);
        let mut msg_rx = event_bus.subscribe();
        tokio_spawn(async move {
            while let Ok(event) = msg_rx.recv().await {
                if let Event::Message(MessageEvent::MessageReceived { message_id, session_id }) = event {
                    if let Ok(Some(message)) = storage_clone.get_message(&message_id).await {
                        let uid = user_id_clone.read().await.clone();
                        if message.sender_id != uid {
                            let _ = session_service_clone.increment_unread(&session_id).await;
                        }
                    }
                }
            }
        });
        
        // 启动事件监听器，通知观察者
        let observer_registry_clone = Arc::clone(&observer_registry);
        let event_bus_for_observer = Arc::clone(&event_bus);
        tokio_spawn(async move {
            let mut rx = event_bus_for_observer.subscribe();
            while let Ok(event) = rx.recv().await {
                let _ = observer_registry_clone.notify_event(&event).await;
            }
        });
        
        Ok(Self {
            connection,
            message_service,
            session_service,
            sync_service,
            storage,
            event_bus,
            config: shared_config,
            user_id: user_id_shared,
            message_frame_handler,
            observer_registry,
            #[cfg(feature = "extensions")]
            #[cfg(feature = "extensions")]
            extension_manager,
        })
    }

    /// 登录（快速模式）
    /// 
    /// 使用预初始化的客户端快速登录，无需重新创建客户端
    /// 
    /// # 参数
    /// - `user_id`: 用户 ID
    /// - `token`: 认证 Token
    /// 
    /// # 返回
    /// - `Result<LoginResult>`: 登录结果
    /// 
    /// # 性能优化
    /// - 连接和认证并行处理，减少等待时间
    /// - 使用事件驱动，避免轮询
    /// - 优化超时设置，快速失败
    pub async fn login(&self, user_id: &str, token: &str) -> Result<LoginResult> {
        use std::time::Instant;
        let login_start = Instant::now();
        info!(user_id = %user_id, "Logging in (fast mode)");
        
        // 1. 更新配置中的 token 和 user_id（快速更新）
        {
            let mut config = self.config.write().await;
            config.token = Some(token.to_string());
            config.user_id = user_id.to_string();
        }
        
        // 2. 并行执行：连接 + 订阅事件（减少等待时间）
        let (protocols_opt, protocol_opt, server_url, connect_timeout) = {
            let config_guard = self.config.read().await;
            (
                config_guard.protocols.clone(),
                config_guard.protocol,
                config_guard.server_url.clone(),
                config_guard.connect_timeout,
            )
        };
        
        // 提前订阅事件，避免错过连接事件
        // 必须在连接之前订阅，确保能收到所有事件
        let mut event_rx = self.event_bus.subscribe();
        
        // 连接到服务器（token 会在 CONNECT 消息中自动发送）
        let connect_future: std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> = if let Some(protocols) = protocols_opt.clone() {
            // 协议竞速模式
            Box::pin(self.connection.connect_with_race(protocols))
        } else {
            // 单协议模式
            let protocol = protocol_opt
                .unwrap_or(flare_core::common::config_types::TransportProtocol::WebSocket);
            Box::pin(self.connection.connect(protocol))
        };
        
        // 3. 等待连接和认证完成
        // 关键修复：参考 flare_chat_client.rs，连接成功后直接检查状态
        // build_with_race() 返回时，CONNECT_ACK 可能已经收到（在 connect_with_race 中已等待 200ms）
        // 或者还在异步处理中，我们需要等待并检查状态
        use tokio::time::{Duration, sleep};
        use tracing::{debug, warn};
        
        // 等待连接完成
        let connect_result = connect_future.await;
        
        // 检查连接结果
        match connect_result {
            Ok(()) => {
                info!("Connection established successfully");
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                // 检查是否是超时错误
                if error_msg.contains("timeout") || error_msg.contains("timed out") || error_msg.contains("Protocol race timed out") {
                    // 检查是否是协议竞速超时
                    if error_msg.contains("Protocol race timed out") {
                        return Err(anyhow::anyhow!("Protocol race timeout: All protocols failed to connect within {} seconds. Please check if the server is running at {} and the network connection is stable.", connect_timeout, server_url))
                            .context("Failed to connect to server: protocol race timeout");
                    }
                    return Err(anyhow::anyhow!("Connection timeout: Unable to connect to server within {} seconds. Please check if the server is running at {} and the network connection is stable.", connect_timeout, server_url))
                        .context("Failed to connect to server: timeout");
                }
                // 检查是否是连接拒绝错误
                if error_msg.contains("Connection refused") || error_msg.contains("refused") || error_msg.contains("ECONNREFUSED") {
                    return Err(anyhow::anyhow!("Connection refused: The server at {} is not accepting connections. Please check if the server is running. You can start the server with: cargo run --example flare_chat_server", server_url))
                        .context("Failed to connect to server: connection refused");
                }
                // 检查是否是 DNS 错误
                if error_msg.contains("DNS") || error_msg.contains("resolve") || error_msg.contains("not found") || error_msg.contains("Name or service not known") {
                    return Err(anyhow::anyhow!("DNS resolution failed: Cannot resolve server address {}. Please check the server URL.", server_url))
                        .context("Failed to connect to server: DNS error");
                }
                // 检查是否是协议错误
                if error_msg.contains("protocol") || error_msg.contains("handshake") {
                    return Err(anyhow::anyhow!("Protocol error: Failed to establish connection with server at {}. Error: {}", server_url, error_msg))
                        .context("Failed to connect to server: protocol error");
                }
                // 其他错误，提供详细错误信息
                return Err(anyhow::anyhow!("Connection failed: {} (Server: {})", error_msg, server_url))
                    .context("Failed to connect to server");
            }
        }
        
        // 连接成功后，等待并检查认证状态
        // 关键修复：增加等待时间，并优先监听事件而不是轮询状态
        // 这样可以更快地响应 CONNECT_ACK 事件
        let auth_check_start = std::time::Instant::now();
        let max_auth_wait = Duration::from_secs(10); // 增加等待时间到 10 秒，给状态更新更多时间
        
        // 先快速检查一次状态（可能状态已经更新）
        let initial_state = self.connection.state().await;
        if matches!(initial_state, crate::connection::ConnectionState::Authenticated) {
            info!("✅ Authentication already completed (state check)");
            // 注意：这里不提前返回，继续执行后续的更新逻辑
            // 因为 login 方法还需要更新服务中的 user_id 等
        }
        
        loop {
            // 优先监听事件（事件可能比状态轮询更快）
            tokio::select! {
                // 接收事件（带超时，避免无限等待）
                event_result = tokio::time::timeout(Duration::from_millis(200), event_rx.recv()) => {
                    match event_result {
                        Ok(Ok(event)) => {
                            match event {
                                Event::Connection(ConnectionEvent::Authenticated) => {
                                    info!("✅ Authentication successful (received Authenticated event)");
                                    break;
                                }
                                Event::Connection(ConnectionEvent::Disconnected) => {
                                    return Err(anyhow::anyhow!("Connection disconnected during authentication"));
                                }
                                Event::Connection(ConnectionEvent::AuthenticationFailed(reason)) => {
                                    return Err(anyhow::anyhow!("Authentication failed: {}", reason));
                                }
                                Event::Connection(ConnectionEvent::Error(err)) => {
                                    // 检查是否是认证相关错误
                                    if err.contains("认证") || err.contains("Token") || err.contains("authentication") {
                                        return Err(anyhow::anyhow!("Authentication error: {}", err));
                                    }
                                    // 其他错误记录但不返回
                                    debug!("Connection event error (non-fatal): {}", err);
                                }
                                _ => {
                                    // 其他事件，继续等待
                                }
                            }
                        }
                        Ok(Err(_)) => {
                            // 事件通道关闭，继续检查状态
                            debug!("Event channel closed, continuing state check");
                        }
                        Err(_) => {
                            // 事件接收超时，继续检查状态
                        }
                    }
                }
                // 定期检查状态（每200ms，与事件超时同步）
                _ = sleep(Duration::from_millis(200)) => {
                    // 检查连接状态
                    let state = self.connection.state().await;
                    let is_connected = self.connection.is_connected().await;
                    
                    // 如果连接已断开，返回错误
                    if !is_connected {
                        return Err(anyhow::anyhow!("Connection lost after establishment"));
                    }
                    
                    // 如果状态已经是 Authenticated，认证成功
                    if matches!(state, crate::connection::ConnectionState::Authenticated) {
                        info!("✅ Authentication verified by connection state");
                        break;
                    }
                    
                    // 检查是否超时
                    if auth_check_start.elapsed() > max_auth_wait {
                        // 超时，但连接仍然有效，检查是否是 Connected 状态（可能认证还在进行中）
                        if matches!(state, crate::connection::ConnectionState::Connected) {
                            warn!("Authentication timeout after {} seconds, but connection is still Connected. Assuming authentication is in progress.", max_auth_wait.as_secs());
                            // 继续等待一小段时间（给状态更新更多时间）
                            tokio::time::sleep(Duration::from_millis(1000)).await;
                            // 再次检查
                            let final_state = self.connection.state().await;
                            if matches!(final_state, crate::connection::ConnectionState::Authenticated) {
                                info!("✅ Authentication completed after additional wait");
                                break;
                            }
                        }
                        return Err(anyhow::anyhow!("Authentication timeout after {} seconds. Connection state: {:?}", 
                            max_auth_wait.as_secs(), state))
                            .context("Authentication failed: timeout");
                    }
                }
            }
        }
        
        // 认证检查已在上面完成，记录日志
        info!(elapsed_ms = login_start.elapsed().as_millis(), "Authentication completed");
        
        // 4. 更新服务中的 user_id（优化：并行更新）
        let user_id_str = user_id.to_string();
        let (_, _, _, _) = tokio::join!(
            self.message_service.set_user_id(user_id_str.clone()),
            self.session_service.set_user_id(user_id_str.clone()),
            self.sync_service.set_user_id(user_id_str.clone()),
            async {
                let mut uid = self.user_id.write().await;
                *uid = user_id_str;
            }
        );
        
        // 5. 启动重连同步监听器（使用默认策略，但不立即同步）
        // 注意：不在登录时立即同步，让用户连接成功后直接进入消息页面
        self.sync_service.start_reconnect_sync_listener(None).await
            .context("Failed to start reconnect sync listener")?;
        
        // 6. 不在登录时启动全量同步，让用户连接成功后直接进入消息页面
        // 同步可以在用户进入消息页面后按需触发
        
        info!(elapsed_ms = login_start.elapsed().as_millis(), "Login completed");
        
        Ok(LoginResult {
            user_id: user_id.to_string(),
            session_id: String::new(), // TODO: 从认证响应中获取
        })
    }

    /// 注册新用户
    /// 
    /// # 参数
    /// - `username`: 用户名（可选，如果使用邮箱注册）
    /// - `email`: 邮箱（可选，如果使用用户名注册）
    /// - `password`: 密码
    /// - `metadata`: 额外元数据（可选）
    /// 
    /// # 返回
    /// - `Result<RegisterResult>`: 注册结果
    /// 
    /// # 注意
    /// - 注册通过HTTP完成（不通过长连接）
    /// - 注册成功后，可以立即使用返回的token登录
    
    
    /// 登录（使用用户名密码，自动获取token）
    /// 
    /// # 参数
    /// - `username`: 用户名或邮箱
    /// - `password`: 密码
    /// 
    /// # 返回
    /// - `Result<LoginResult>`: 登录结果
    /// 
    /// # 注意
    /// - 此方法会先通过HTTP获取token，然后使用token通过长连接登录
    /// - 登录成功后会自动执行全量同步
    

    pub async fn set_crypto_aes256(&self, key: &[u8]) -> Result<()> {
        let crypto = crate::service::AesCrypto::new(key)?;
        self.message_service.set_crypto(Arc::new(crypto)).await;
        Ok(())
    }

    pub async fn set_crypto(&self, crypto: Arc<dyn crate::service::CryptoService>) -> Result<()> {
        self.message_service.set_crypto(crypto).await;
        Ok(())
    }
    
    /// 登出
    pub async fn logout(&self) -> Result<()> {
        info!("Logging out");
        
        // 1. 断开连接
        self.connection.disconnect().await
            .context("Failed to disconnect")?;
        
        // 2. TODO: 发送登出请求
        
        // 3. 清除保存的凭证（如果使用存储便捷API）
        
        Ok(())
    }
    

    /// 发送消息
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `content`: 消息内容
    /// 
    /// # 返回
    /// - `Result<String>`: 消息 ID
    pub async fn send_message(
        &self,
        session_id: &str,
        content: flare_proto::MessageContent,
    ) -> Result<String> {
        self.message_service.send_message(session_id, content).await
            .context("Failed to send message")
    }

    pub async fn reply_message(
        &self,
        session_id: &str,
        reply_to_message_id: &str,
        content: flare_proto::MessageContent,
    ) -> Result<String> {
        self.message_service
            .reply_message(session_id, reply_to_message_id, content)
            .await
            .context("Failed to send reply message")
    }

    pub async fn add_thread_reply(
        &self,
        session_id: &str,
        thread_id: &str,
        content: flare_proto::MessageContent,
    ) -> Result<String> {
        self.message_service
            .add_thread_reply(session_id, thread_id, content)
            .await
            .context("Failed to send thread reply")
    }

    pub async fn mark_session_read(
        &self,
        session_id: &str,
        message_seq: Option<i64>,
    ) -> Result<()> {
        self.session_service
            .mark_as_read(session_id, message_seq)
            .await
            .context("Failed to mark session as read")
    }

    pub async fn recall_message(&self, message_id: &str) -> Result<()> {
        self.message_service
            .recall_message(message_id)
            .await
            .context("Failed to recall message")
    }

    pub async fn add_reaction(&self, session_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        self.message_service.add_reaction(session_id, message_id, emoji).await
            .context("Failed to add reaction")
    }

    pub async fn remove_reaction(&self, session_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        self.message_service.remove_reaction(session_id, message_id, emoji).await
            .context("Failed to remove reaction")
    }

    pub async fn edit_message(&self, session_id: &str, message_id: &str, content: flare_proto::MessageContent, attributes: Option<std::collections::HashMap<String, String>>) -> Result<()> {
        self.message_service.edit_message(session_id, message_id, content, attributes).await
            .context("Failed to edit message")
    }

    /// 获取会话列表
    /// 
    /// # 参数
    /// - `filter`: 会话过滤条件
    /// 
    /// # 返回
    /// - `Result<Vec<SessionSummary>>`: 会话列表
    pub async fn get_sessions(
        &self,
        filter: SessionFilter,
    ) -> Result<Vec<SessionSummary>> {
        self.session_service.get_sessions(filter).await
            .context("Failed to get sessions")
    }
    
    /// 获取会话列表（带扩展信息）
    /// 
    /// # 参数
    /// - `filter`: 会话过滤条件
    /// 
    /// # 返回
    /// - `Result<Vec<ExtendedSessionSummary>>`: 带扩展信息的会话列表
    /// 
    /// # 注意
    /// 需要启用 `extensions` feature
    #[cfg(feature = "extensions")]
    pub async fn get_sessions_extended(
        &self,
        filter: SessionFilter,
    ) -> Result<Vec<ExtendedSessionSummary>> {
        self.session_service.get_sessions_extended(filter).await
            .context("Failed to get sessions with extensions")
    }
    
    /// 获取会话详情（带扩展信息）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// 
    /// # 返回
    /// - `Result<ExtendedSessionSummary>`: 带扩展信息的会话详情
    /// 
    /// # 注意
    /// 需要启用 `extensions` feature
    #[cfg(feature = "extensions")]
    pub async fn get_session_extended(
        &self,
        session_id: &str,
    ) -> Result<ExtendedSessionSummary> {
        self.session_service.get_session_extended(session_id).await
            .context("Failed to get session with extensions")
    }

    /// 获取会话消息
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 返回的最大消息数量
    /// - `cursor`: 可选游标，用于分页
    /// 
    /// # 返回
    /// - `Result<Vec<Message>>`: 消息列表
    pub async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<Message>> {
        self.message_service.get_local_messages(session_id, limit, cursor).await
            .context("Failed to get messages")
    }
    
    /// 获取消息列表（带扩展信息）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `limit`: 返回的最大消息数量
    /// - `cursor`: 可选游标，用于分页
    /// 
    /// # 返回
    /// - `Result<Vec<ExtendedMessage>>`: 带扩展信息的消息列表
    /// 
    /// # 注意
    /// 需要启用 `extensions` feature
    #[cfg(feature = "extensions")]
    pub async fn get_messages_extended(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<ExtendedMessage>> {
        self.message_service.get_local_messages_extended(session_id, limit, cursor).await
            .context("Failed to get messages with extensions")
    }
    
    /// 注册扩展提供者
    /// 
    /// # 参数
    /// - `provider`: 扩展提供者
    /// 
    /// # 注意
    /// 需要启用 `extensions` feature
    /// 
    /// # 示例
    /// ```rust,no_run
    /// use flare_im_core_sdk::extension::MemoryExtensionProvider;
    /// 
    /// let provider = Arc::new(MemoryExtensionProvider::new());
    /// client.register_extension_provider(provider).await;
    /// ```
    #[cfg(feature = "extensions")]
    pub async fn register_extension_provider(
        &self,
        provider: Arc<dyn crate::model::ExtensionProvider>,
    ) -> Result<()> {
        #[cfg(feature = "extensions")]
        {
            self.extension_manager.add_provider(provider).await;
            Ok(())
        }
        #[cfg(not(feature = "extensions"))]
        {
            Err(anyhow::anyhow!("Extensions feature not enabled"))
        }
    }
    
    /// 设置扩展缓存
    /// 
    /// # 参数
    /// - `cache`: 扩展缓存
    /// 
    /// # 注意
    /// 需要启用 `extensions` feature
    /// 
    /// # 示例
    /// ```rust,no_run
    /// use flare_im_core_sdk::extension::MemoryExtensionCache;
    /// 
    /// let cache = Arc::new(MemoryExtensionCache::new(300)); // 5分钟TTL
    /// client.set_extension_cache(cache).await?;
    /// ```
    #[cfg(feature = "extensions")]
    pub async fn set_extension_cache(
        &self,
        cache: Arc<dyn crate::model::ExtensionCache>,
    ) -> Result<()> {
        #[cfg(feature = "extensions")]
        {
            // ExtensionManager 需要支持运行时设置缓存
            // 这里暂时返回错误，后续可以改进
            Err(anyhow::anyhow!("Setting cache at runtime not yet supported. Please set cache when creating ExtensionManager."))
        }
        #[cfg(not(feature = "extensions"))]
        {
            Err(anyhow::anyhow!("Extensions feature not enabled"))
        }
    }

    /// 同步消息（增量）
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `after_seq`: 同步此序列号之后的消息（可选）
    /// 
    /// # 返回
    /// - `Result<crate::model::SyncResult>`: 同步结果
    pub async fn sync_messages(
        &self,
        session_id: &str,
        after_seq: Option<i64>,
    ) -> Result<crate::model::SyncResult> {
        self.sync_service.sync_messages(session_id, after_seq).await
            .context("Failed to sync messages")
    }

    /// 同步会话（增量/全量）
    /// 
    /// # 参数
    /// - `cursor`: 可选游标，用于增量同步
    /// 
    /// # 返回
    /// - `Result<crate::service::SessionSyncResult>`: 同步结果
    pub async fn sync_sessions(
        &self,
        cursor: Option<String>,
    ) -> Result<crate::service::SessionSyncResult> {
        self.sync_service.sync_sessions(cursor).await
            .context("Failed to sync sessions")
    }

    /// 标记消息已读
    /// 
    /// # 参数
    /// - `session_id`: 会话 ID
    /// - `message_seq`: 已读到的消息序列号（可选）
    pub async fn mark_as_read(
        &self,
        session_id: &str,
        message_seq: Option<i64>,
    ) -> Result<()> {
        self.session_service.mark_as_read(session_id, message_seq).await
            .context("Failed to mark as read")
    }

    /// 注册消息观察者（统一的消息处理接口）
    /// 
    /// # 参数
    /// - `observer`: 消息观察者
    pub async fn register_message_observer(
        &self,
        observer: crate::observer::ArcMessageObserver,
    ) {
        // 同时注册到 MessageService 和 ObserverRegistry
        self.message_service.register_observer(Arc::clone(&observer)).await;
        self.observer_registry.register(observer).await;
    }

    // 事件监听请使用 `event_bus().subscribe()` 获取接收器并自行处理

    /// 获取连接状态
    pub async fn connection_state(&self) -> crate::connection::ConnectionState {
        self.connection.state().await
    }

    /// 获取事件总线（用于高级用法）
    pub fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }
    
    /// 设置扩展管理器
    /// 
    /// # 参数
    /// - `manager`: 扩展管理器
    /// 
    /// # 注意
    /// - 此方法会初始化所有已注册的扩展点
    /// - 由于FlareIMClient使用Arc，此方法返回新的客户端实例
    
    
    /// 获取存储后端（用于高级用法）
    pub fn storage(&self) -> Arc<dyn crate::storage::StorageBackend> {
        Arc::clone(&self.storage)
    }

    pub fn message_service(&self) -> Arc<crate::service::MessageService> {
        Arc::clone(&self.message_service)
    }

    pub async fn user_id(&self) -> Result<String> {
        Ok(self.user_id.read().await.clone())
    }

    pub async fn create_session(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
    ) -> Result<String> {
        self.session_service.create_session(session_id, session_type, business_type, display_name).await
            .context("Failed to create session")
    }
}
