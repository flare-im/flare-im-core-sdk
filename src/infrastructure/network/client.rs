use std::sync::Arc;
use std::time::Duration;
use flare_core::client::builder::flare::{FlareClient, FlareClientBuilder};
use flare_core::common::protocol::Frame;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn, debug};

use super::types::{NetworkMessage, ConnectionEvent};
use super::listener::NetworkMessageListener;
use super::parser;

pub struct NetworkClient {
    client: Option<Arc<Mutex<FlareClient>>>,
    message_tx: mpsc::UnboundedSender<NetworkMessage>,
    connection_tx: mpsc::UnboundedSender<ConnectionEvent>,
    #[allow(dead_code)]
    ack_tx: Option<mpsc::UnboundedSender<Frame>>,
}

impl NetworkClient {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<NetworkMessage>, mpsc::UnboundedReceiver<ConnectionEvent>) {
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let (connection_tx, connection_rx) = mpsc::unbounded_channel();
        
        let client = Self {
            client: None,
            message_tx,
            connection_tx,
            ack_tx: None,
        };
        
        (client, message_rx, connection_rx)
    }
    
    pub fn new_with_queue(
        queue: std::sync::Arc<crate::domain::message_queue::MessageQueue>,
        event_bus: std::sync::Arc<crate::infrastructure::event_bus::EventBus>,
    ) -> (Self, mpsc::UnboundedReceiver<NetworkMessage>, mpsc::UnboundedReceiver<ConnectionEvent>, mpsc::UnboundedReceiver<Frame>) {
        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let (connection_tx, connection_rx) = mpsc::unbounded_channel();
        let (ack_tx, ack_rx) = mpsc::unbounded_channel();
        let (external_tx, external_rx) = mpsc::unbounded_channel();
        let message_tx_for_external = external_tx.clone();
        let queue_clone = queue.clone();
        let ack_tx_clone = ack_tx.clone();
        let event_stream_processor = crate::infrastructure::event_stream::EventStreamProcessor::new(queue_clone.clone(), event_bus);
        tokio::spawn(async move {
            while let Some(network_msg) = message_rx.recv().await {
                match network_msg {
                    NetworkMessage::EventEnvelope(env) => {
                        if let Err(e) = event_stream_processor.process(&env).await {
                            tracing::warn!(error = %e, event_count = env.events.len(), "EventStreamProcessor process envelope failed");
                        }
                    }
                    NetworkMessage::Received(frame) => {
                        // 检查是否是 ACK 消息
                        let is_ack = frame.command.as_ref()
                            .and_then(|c| c.r#type.as_ref())
                            .and_then(|t| {
                                if let flare_core::common::protocol::flare::core::commands::command::Type::Message(mc) = t {
                                    Some(mc.r#type == 1) // Type::Ack = 1
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(false);
                        
                        if is_ack {
                            let _ = ack_tx_clone.send(frame.clone());
                        } else {
                            // 普通消息解析并加入队列
                            // Type::Send(0) 为历史推送；Type::Data(2) 为新 ServerPacket 推送/同步
                            let is_message_frame = frame.command.as_ref()
                                .and_then(|cmd| cmd.r#type.as_ref())
                                .and_then(|t| {
                                    if let flare_core::common::protocol::flare::core::commands::command::Type::Message(mc) = t {
                                        Some(mc.r#type == 0 || mc.r#type == 2)
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(false);
                            
                            if is_message_frame {
                                debug!(frame_id = %frame.message_id, "Received message frame, parsing...");
                                match parser::parse_frame_to_messages(&frame) {
                                    Ok(messages) => {
                                        if messages.is_empty() {
                                            debug!(frame_id = %frame.message_id, "Parsed 0 messages from frame");
                                            continue;
                                        }

                                        let priority = 10u8;
                                        for message in messages {
                                            let message_id = message
                                                .server_id
                                                .as_ref()
                                                .map(|s| s.to_string())
                                                .unwrap_or_else(|| message.client_msg_id.clone());
                                            let conversation_id_str = message
                                                .conversation_id
                                                .as_ref()
                                                .map(|s| s.as_str())
                                                .unwrap_or("<none>")
                                                .to_string();
                                            let sender_id = message.sender_id.clone();

                                            match queue_clone.enqueue(message, priority).await {
                                                true => {
                                                    debug!(
                                                        message_id = %message_id,
                                                        conversation_id = %conversation_id_str,
                                                        sender_id = %sender_id,
                                                        "Message enqueued successfully"
                                                    );
                                                }
                                                false => {
                                                    let is_dup = queue_clone.is_duplicate(&message_id).await;
                                                    if is_dup {
                                                        debug!(
                                                            message_id = %message_id,
                                                            conversation_id = %conversation_id_str,
                                                            sender_id = %sender_id,
                                                            "Message is duplicate, skipping"
                                                        );
                                                    } else {
                                                        warn!(
                                                            message_id = %message_id,
                                                            conversation_id = %conversation_id_str,
                                                            sender_id = %sender_id,
                                                            "Failed to enqueue received message (queue may be full)"
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            frame_id = %frame.message_id,
                                            "Failed to parse frame to messages"
                                        );
                                    }
                                }
                            } else {
                                debug!(frame_id = %frame.message_id, "Received non-message frame, skipping");
                            }
                        }
                    }
                    // 其他类型的消息：转发到外部通道，由 NetworkMessageDispatcher 处理
                    msg @ NetworkMessage::SyncMessages(_)
                    | msg @ NetworkMessage::SyncConversations(_)
                    | msg @ NetworkMessage::ConversationSyncAll(_)
                    | msg @ NetworkMessage::ConversationDetail(_)
                    | msg @ NetworkMessage::CustomPushData { .. }
                    | msg @ NetworkMessage::Connected(_)
                    | msg @ NetworkMessage::Disconnected(_)
                    | msg @ NetworkMessage::Error(_) => {
                        // 转发到外部通道，由 NetworkMessageDispatcher 处理
                        if let Err(e) = message_tx_for_external.send(msg) {
                            error!("Failed to forward network message: {}", e);
                        }
                    }
                }
            }
        });
        
        let client = Self {
            client: None,
            message_tx, // 内部使用 message_tx 发送所有消息
            connection_tx,
            ack_tx: Some(ack_tx), // 保留以保持接口兼容，但不再使用
        };
        
        (client, external_rx, connection_rx, ack_rx)
    }
    
    /// 连接到服务器（使用简单配置）
    ///
    /// # 参数
    ///
    /// * `server_url` - 服务器地址
    /// * `user_id` - 用户 ID
    /// * `token` - 认证 Token
    pub async fn connect(
        &mut self,
        server_url: String,
        user_id: String,
        token: String,
    ) -> anyhow::Result<()> {
        self.connect_with_config(server_url, user_id, token, None).await
    }
    
    /// 连接到服务器（使用完整配置）
    ///
    /// 对标微信、Telegram、飞书的连接机制
    ///
    /// # 参数
    ///
    /// * `server_url` - 服务器地址
    /// * `user_id` - 用户 ID
    /// * `token` - 认证 Token
    /// * `config` - 客户端配置（可选）
    pub async fn connect_with_config(
        &mut self,
        server_url: String,
        user_id: String,
        token: String,
        config: Option<flare_core::client::config::ClientConfig>,
    ) -> anyhow::Result<()> {
        info!("连接到服务器: {}", server_url);
        
        // 创建消息监听器
        let listener = Arc::new(NetworkMessageListener::new(
            self.message_tx.clone(),
            self.connection_tx.clone(),
        ));
        
        // 构建客户端（使用 Flare 模式）
        use flare_core::common::device::{DeviceInfo, DevicePlatform};
        
        // 创建默认设备信息（PC 平台）
        let default_device = DeviceInfo::new(
            format!("sdk-device-{}", std::process::id()),
            DevicePlatform::PC,
        )
        .with_model("SDK-Client".to_string())
        .with_app_version("1.0.0".to_string())
        .with_system_version("Unknown".to_string());
        
        let mut builder = FlareClientBuilder::new(&server_url)
            .with_user_id(user_id.clone())
            .with_token(token)
            .with_listener(listener)
            .with_device_info(default_device); // 设置设备信息（Flare 模式必需）
        
        // 如果提供了完整配置，应用配置
        if let Some(cfg) = config {
            // 如果配置中有设备信息，使用配置的设备信息（覆盖默认值）
            if let Some(ref device_info) = cfg.device_info {
                builder = builder.with_device_info(device_info.clone());
            }
            
            // 应用序列化格式和压缩（但不使用加密）
            if let Some(format) = cfg.force_serialization_format {
                builder = builder.force_format(format);
            } else {
                // 使用配置中的格式（如果配置了）
                builder = builder.with_format(cfg.serialization_format);
            }
            
            if let Some(compression) = cfg.force_compression {
                builder = builder.force_compression(compression);
            } else {
                // 使用配置中的压缩（如果配置了）
                builder = builder.with_compression(cfg.compression);
            }
            
            // 应用协议配置
            if let Some(ref protocol_urls) = cfg.protocol_urls {
                use flare_core::common::config_types::TransportProtocol as CoreTransportProtocol;
                for (protocol, url) in protocol_urls {
                    match protocol {
                        CoreTransportProtocol::WebSocket => {
                            builder = builder.with_protocol_url(
                                flare_core::common::config_types::TransportProtocol::WebSocket,
                                url.clone(),
                            );
                        }
                        CoreTransportProtocol::QUIC => {
                            builder = builder.with_protocol_url(
                                flare_core::common::config_types::TransportProtocol::QUIC,
                                url.clone(),
                            );
                        }
                        CoreTransportProtocol::TCP => {
                            // TCP 协议暂不支持，跳过
                            warn!("TCP 协议暂不支持，跳过配置");
                        }
                    }
                }
            }
            
            // 应用协议竞速配置
            if let Some(ref transports) = cfg.transports {
                use flare_core::common::config_types::TransportProtocol as CoreTransportProtocol;
                let protocols: Vec<flare_core::common::config_types::TransportProtocol> = transports
                    .iter()
                    .filter_map(|p| match p {
                        CoreTransportProtocol::WebSocket => Some(flare_core::common::config_types::TransportProtocol::WebSocket),
                        CoreTransportProtocol::QUIC => Some(flare_core::common::config_types::TransportProtocol::QUIC),
                        CoreTransportProtocol::TCP => {
                            // TCP 协议暂不支持，过滤掉
                            warn!("TCP 协议暂不支持，从协议列表中移除");
                            None
                        }
                    })
                    .collect();
                if !protocols.is_empty() {
                    builder = builder.with_protocol_race(protocols);
                }
            }
            
            // 应用超时配置
            builder = builder.with_connect_timeout(cfg.connect_timeout);
            
            // 应用重连配置
            builder = builder.with_reconnect_interval(cfg.reconnect_interval);
            if let Some(attempts) = cfg.max_reconnect_attempts {
                builder = builder.with_max_reconnect_attempts(Some(attempts));
            }
            
            // 应用心跳配置（heartbeat 是 HeartbeatConfig，直接使用）
            builder = builder.with_heartbeat(cfg.heartbeat.clone());
        }
        
        // 构建并连接客户端
        let client = builder
            .build_with_race()
            .await
            .map_err(|e| anyhow::anyhow!("连接失败: {}", e))?;
        
        // 使用 Arc<Mutex<>> 包装，以便可以调用 send_frame_and_wait
        self.client = Some(Arc::new(Mutex::new(client)));
        
        info!("✅ 已成功连接到服务器: {}", server_url);
        Ok(())
    }
    
    /// 断开连接
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        if let Some(client) = self.client.take() {
            // FlareClient::disconnect 需要所有权，所以这里直接 drop
            // 客户端会自动断开连接
            drop(client);
            info!("Disconnected from server");
        }
        Ok(())
    }
    
    /// 发送消息 Frame
    ///
    /// # 参数
    ///
    /// * `frame` - 要发送的 Frame
    pub async fn send_frame(&self, frame: &Frame) -> anyhow::Result<()> {
        let client = self.client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Client is not connected"))?;
        
        let client_guard = client.lock().await;
        client_guard.send_frame(frame).await
            .map_err(|e| anyhow::anyhow!("Failed to send frame: {}", e))?;
        
        Ok(())
    }
    
    /// 发送 Frame 并等待响应
    ///
    /// 使用 flare-core 的 `send_frame_and_wait`，自动处理 ACK 匹配和超时
    /// 这是推荐的发送方式，替代自定义的 ack_waiters 机制
    ///
    /// # 参数
    ///
    /// * `frame` - 要发送的 Frame
    /// * `timeout` - 超时时间
    ///
    /// # 返回
    ///
    /// * `Ok(Frame)` - 收到响应
    /// * `Err` - 发送失败或超时
    pub async fn send_frame_and_wait(
        &self,
        frame: &Frame,
        timeout: Duration,
    ) -> anyhow::Result<Frame> {
        let client = self.client.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Client is not connected"))?;
        
        let client_guard = client.lock().await;
        client_guard.send_frame_and_wait(frame, timeout).await
            .map_err(|e| anyhow::anyhow!("Failed to send frame and wait: {}", e))
    }
    
    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.client.as_ref()
            .map(|c| {
                // FlareClient::is_connected 不需要 &mut，可以直接调用
                // 但需要通过 Mutex 访问
                tokio::task::block_in_place(|| {
                    let guard = c.blocking_lock();
                    guard.is_connected()
                })
            })
            .unwrap_or(false)
    }
    
    /// 获取连接 ID
    pub fn connection_id(&self) -> Option<String> {
        self.client.as_ref()
            .and_then(|c| {
                tokio::task::block_in_place(|| {
                    let guard = c.blocking_lock();
                    guard.connection_id()
                })
            })
    }
}

impl Default for NetworkClient {
    fn default() -> Self {
        let (client, _rx1, _rx2) = Self::new();
        client
    }
}
