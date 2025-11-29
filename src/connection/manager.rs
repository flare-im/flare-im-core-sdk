//! 连接管理器

use flare_core::client::builder::flare::{FlareClientBuilder, FlareClient};
use flare_core::common::config_types::TransportProtocol;
use crate::connection::message_listener::SDKMessageListener;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::transport::events::{ConnectionObserver, ConnectionEvent as FlareConnectionEvent};
use crate::config::ClientConfig;
use crate::event::{EventBus, Event, ConnectionEvent};
use crate::connection::state_machine::{ConnectionStateMachine, StateTransition, StateMachineConfig};
use anyhow::Context;
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use std::time::Duration;

/// 连接状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Authenticating,
    Authenticated,
    Reconnecting,
}

/// 连接管理器
pub struct ConnectionManager {
    #[cfg(not(target_arch = "wasm32"))]
    client: Arc<Mutex<Option<FlareClient>>>,
    config: Arc<tokio::sync::RwLock<ClientConfig>>,
    /// 状态机（管理状态转换）
    state_machine: Arc<ConnectionStateMachine>,
    /// 旧的状态字段（保持兼容性，通过状态机访问）
    #[deprecated(note = "Use state_machine instead")]
    state: Arc<RwLock<ConnectionState>>,
    active_protocol: Arc<RwLock<Option<TransportProtocol>>>,
    event_bus: Arc<EventBus>,
    reconnect_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// 消息监听器（用于 FlareClientBuilder）
    message_listener: Arc<Mutex<Option<Arc<SDKMessageListener>>>>,
}

/// 连接事件观察者
/// 
/// 将 flare-core 的连接事件转换为 SDK 的事件
#[cfg(not(target_arch = "wasm32"))]
struct ConnectionEventObserver {
    event_bus: Arc<EventBus>,
    state_machine: Arc<ConnectionStateMachine>,
    state: Arc<RwLock<ConnectionState>>, // 保持兼容性
    active_protocol: Arc<RwLock<Option<TransportProtocol>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ConnectionObserver for ConnectionEventObserver {
    fn on_event(&self, event: &FlareConnectionEvent) {
        // 关键修复：避免使用 block_on，改用异步任务但确保状态更新及时完成
        // block_on 可能导致死锁（如果当前任务持有锁）或延迟执行
        // 改用高优先级异步任务，确保状态更新及时完成
        
        let event_clone = event.clone();
        let state_machine = Arc::clone(&self.state_machine);
        let state = Arc::clone(&self.state);
        let active_protocol = Arc::clone(&self.active_protocol);
        let event_bus = Arc::clone(&self.event_bus);
        
        // 使用 spawn 创建异步任务，但使用高优先级确保及时执行
        // 对于 CONNECT_ACK，我们需要立即更新状态，所以使用 spawn_blocking 或确保任务立即执行
        tokio::spawn(async move {
            // 立即处理事件，确保状态更新及时完成
            match &event_clone {
                FlareConnectionEvent::Connected => {
                    // 使用状态机进行状态转换
                    let current_state = state_machine.current_state().await;
                    if current_state != ConnectionState::Authenticated {
                        if let Err(e) = state_machine.transition(StateTransition::Connected).await {
                            tracing::warn!(error = %e, "Failed to transition to Connected state");
                        } else {
                            // 同步到旧的状态字段
                            *state.write().await = ConnectionState::Connected;
                        }
                    }
                    tracing::debug!(
                        "Connection established, waiting for authentication"
                    );
                }
                FlareConnectionEvent::Disconnected(reason) => {
                    let previous_state = state_machine.current_state().await;
                    // 使用状态机进行状态转换（会自动发布事件）
                    if let Err(e) = state_machine.transition(StateTransition::Disconnect).await {
                        tracing::warn!(error = %e, "Failed to transition to Disconnected state");
                    } else {
                        // 同步到旧的状态字段
                        *state.write().await = ConnectionState::Disconnected;
                    }
                    *active_protocol.write().await = None;
                    
                    // 检查是否是过早断开
                    if matches!(previous_state, ConnectionState::Connected | ConnectionState::Authenticating) {
                        tracing::warn!(
                            reason = %reason,
                            previous_state = ?previous_state,
                            "连接建立后立即断开，可能是认证失败或服务器问题"
                        );
                    }
                    
                    tracing::info!("连接已断开: {}", reason);
                }
                FlareConnectionEvent::Message(data) => {
                    // 解析 Frame（同步操作）
                    use flare_core::common::MessageParser;
                    use flare_core::common::protocol::SerializationFormat;
                    use flare_core::common::compression::CompressionAlgorithm;
                    use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemCommandType;
                    
                    tracing::debug!("ConnectionEventObserver received Message event, parsing frame...");
                    
                    let frame_result = MessageParser::new(
                        SerializationFormat::Protobuf,
                        CompressionAlgorithm::None,
                    ).parse(data);
                    
                    if let Ok(frame) = frame_result {
                        tracing::debug!("Frame parsed successfully, checking for CONNECT_ACK...");
                        
                        // 检查是否是 CONNECT_ACK，如果是，立即更新状态为 Authenticated
                        if let Some(cmd) = frame.command.as_ref() {
                            if let Some(flare_core::common::protocol::flare::core::commands::command::Type::System(sys_cmd)) = cmd.r#type.as_ref() {
                                if sys_cmd.r#type == SystemCommandType::ConnectAck as i32 {
                                    tracing::info!("CONNECT_ACK detected in ConnectionEventObserver, updating state...");
                                    
                                    // 关键修复：立即更新状态，确保状态更新及时完成
                                    let current_state = state_machine.current_state().await;
                                    if current_state != ConnectionState::Authenticated {
                                        if let Err(e) = state_machine.transition(StateTransition::Authenticated).await {
                                            tracing::warn!(error = %e, "Failed to transition to Authenticated state");
                                        } else {
                                            // 同步到旧的状态字段
                                            *state.write().await = ConnectionState::Authenticated;
                                            tracing::info!(
                                                "Connection state updated to Authenticated (CONNECT_ACK received in ConnectionEventObserver)"
                                            );
                                            
                                            // 立即发布 Authenticated 事件（状态机已发布，但这里也发布以确保）
                                            event_bus.publish(Event::Connection(
                                                ConnectionEvent::Authenticated
                                            ));
                                            tracing::info!(
                                                "Authenticated event published from ConnectionEventObserver"
                                            );
                                        }
                                    } else {
                                        tracing::debug!(
                                            "Connection state already Authenticated"
                                        );
                                    }
                                } else {
                                    tracing::debug!("System command is not CONNECT_ACK, type: {}", sys_cmd.r#type);
                                }
                            } else {
                                tracing::debug!("Command is not a System command");
                            }
                        } else {
                            tracing::debug!("Frame has no command");
                        }
                        
                        // 发布 Frame 事件（异步处理，不阻塞当前任务）
                        let event_bus_clone = Arc::clone(&event_bus);
                        let frame_clone = frame.clone();
                        tokio::spawn(async move {
                            let cmd_name = frame_clone.command.as_ref()
                                .and_then(|c| c.r#type.as_ref())
                                .map(|t| match t {
                                    flare_core::common::protocol::flare::core::commands::command::Type::Message(_) => "Message",
                                    flare_core::common::protocol::flare::core::commands::command::Type::System(_) => "System",
                                    flare_core::common::protocol::flare::core::commands::command::Type::Custom(custom) => custom.name.as_str(),
                                    flare_core::common::protocol::flare::core::commands::command::Type::Notification(_) => "Notification",
                                })
                                .unwrap_or("<none>");
                            let meta_keys: Vec<&str> = frame_clone.metadata.keys().map(|k| k.as_str()).collect();
                            tracing::debug!(msg_id = %frame_clone.message_id, cmd = %cmd_name, meta_keys = ?meta_keys, "Received frame");
                            event_bus_clone.publish(Event::Connection(
                                ConnectionEvent::FrameReceived(frame_clone)
                            ));
                        });
                    } else {
                        tracing::warn!(
                            error = ?frame_result.err(),
                            "Failed to parse message frame"
                        );
                    }
                }
                FlareConnectionEvent::Error(err) => {
                    // 发布错误事件（异步处理）
                    let event_bus_clone = Arc::clone(&event_bus);
                    let err_str = err.to_string();
                    tokio::spawn(async move {
                        event_bus_clone.publish(Event::Connection(
                            ConnectionEvent::Error(err_str)
                        ));
                    });
                    tracing::error!(
                        error = %err,
                        "Connection error"
                    );
                }
            }
        });
    }
}

// 辅助方法：异步处理事件（用于没有运行时的情况）
#[cfg(not(target_arch = "wasm32"))]
impl ConnectionEventObserver {
    async fn handle_event_async(
        event: FlareConnectionEvent,
        state: Arc<RwLock<ConnectionState>>,
        active_protocol: Arc<RwLock<Option<TransportProtocol>>>,
        event_bus: Arc<EventBus>,
    ) {
        match event {
            FlareConnectionEvent::Connected => {
                let current_state = *state.read().await;
                if current_state != ConnectionState::Authenticated {
                    *state.write().await = ConnectionState::Connected;
                }
                tracing::debug!("Connection established, waiting for authentication...");
            }
            FlareConnectionEvent::Disconnected(reason) => {
                let previous_state = *state.read().await;
                *state.write().await = ConnectionState::Disconnected;
                *active_protocol.write().await = None;
                
                if matches!(previous_state, ConnectionState::Connected | ConnectionState::Authenticating) {
                    tracing::warn!(
                        reason = %reason,
                        previous_state = ?previous_state,
                        "连接建立后立即断开，可能是认证失败或服务器问题"
                    );
                }
                
                event_bus.publish(Event::Connection(ConnectionEvent::Disconnected));
                tracing::info!("连接已断开: {}", reason);
            }
            FlareConnectionEvent::Message(data) => {
                // 处理消息（与上面相同）
                use flare_core::common::MessageParser;
                use flare_core::common::protocol::SerializationFormat;
                use flare_core::common::compression::CompressionAlgorithm;
                use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemCommandType;
                
                if let Ok(frame) = MessageParser::new(
                    SerializationFormat::Protobuf,
                    CompressionAlgorithm::None,
                ).parse(&data) {
                    if let Some(cmd) = frame.command.as_ref() {
                        if let Some(flare_core::common::protocol::flare::core::commands::command::Type::System(sys_cmd)) = cmd.r#type.as_ref() {
                            if sys_cmd.r#type == SystemCommandType::ConnectAck as i32 {
                                let current_state = *state.read().await;
                                if current_state != ConnectionState::Authenticated {
                                    *state.write().await = ConnectionState::Authenticated;
                                    tracing::info!(
                                        "Connection state updated to Authenticated (CONNECT_ACK received)"
                                    );
                                    event_bus.publish(Event::Connection(ConnectionEvent::Authenticated));
                                }
                            }
                        }
                    }
                    
                    event_bus.publish(Event::Connection(ConnectionEvent::FrameReceived(frame)));
                }
            }
            FlareConnectionEvent::Error(err) => {
                event_bus.publish(Event::Connection(ConnectionEvent::Error(err.to_string())));
                tracing::error!(
                    error = %err,
                    "Connection error"
                );
            }
        }
    }
}

impl ConnectionManager {
    /// 创建连接管理器
    pub fn new(config: Arc<tokio::sync::RwLock<ClientConfig>>, event_bus: Arc<EventBus>) -> Self {
        // 创建状态机
        let state_machine = Arc::new(ConnectionStateMachine::new(
            ConnectionState::Disconnected,
            Some(Arc::clone(&event_bus)),
            StateMachineConfig::default(),
        ));
        
        // 保持旧的状态字段以兼容现有代码（通过状态机同步）
        let state = Arc::new(RwLock::new(ConnectionState::Disconnected));
        
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            client: Arc::new(Mutex::new(None)),
            config,
            state_machine,
            state,
            active_protocol: Arc::new(RwLock::new(None)),
            event_bus,
            reconnect_handle: Arc::new(Mutex::new(None)),
            message_listener: Arc::new(Mutex::new(None)),
        }
    }
    
    /// 设置消息监听器（用于 FlareClientBuilder）
    /// 
    /// # 已废弃
    /// 
    /// 此方法已废弃，因为创建新运行时可能导致资源浪费和死锁风险。
    /// 请使用异步方法 `set_message_listener` 替代。
    /// 
    /// # 注意
    /// 
    /// 如果必须在同步上下文中使用，请确保：
    /// 1. 当前在 tokio 运行时上下文中
    /// 2. 不会导致死锁（避免在持有锁时调用）
    #[deprecated(note = "请使用异步方法 set_message_listener 替代")]
    pub fn with_message_listener(self, listener: Arc<SDKMessageListener>) -> Self {
        // 使用 tokio::runtime::Handle 来在同步上下文中执行异步操作
        // 注意：这需要运行时可用，且不能持有锁
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // 注意：block_on 可能导致死锁，如果当前任务持有锁
            // 建议使用异步方法 set_message_listener
            handle.block_on(async {
                *self.message_listener.lock().await = Some(listener);
            });
        } else {
            // 如果没有运行时，记录错误但不创建新运行时（避免资源浪费）
            tracing::error!("Cannot set message listener: no tokio runtime available. Please use set_message_listener in async context.");
            // 注意：这里不设置 listener，调用者应该使用异步方法
        }
        self
    }
    
    /// 设置消息监听器（异步方法，用于已创建的 ConnectionManager）
    pub async fn set_message_listener(&self, listener: Arc<SDKMessageListener>) {
        *self.message_listener.lock().await = Some(listener);
    }
    
    /// 更新配置（用于动态更新 token 等）
    pub async fn update_config(&self, f: impl FnOnce(&mut ClientConfig)) {
        let mut config = self.config.write().await;
        f(&mut config);
    }
    
    /// 连接到服务器（协议竞速模式）
    /// 
    /// 同时尝试多个协议，自动选择最快的连接
    /// 
    /// 完全参照 flare_chat_client.rs 示例，使用 FlareClientBuilder 构建客户端
    /// 
    /// 注意：会根据平台自动过滤不支持的协议
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn connect_with_race(&self, protocols: Vec<TransportProtocol>) -> anyhow::Result<()> {
        // 使用状态机进行状态转换
        self.set_state(StateTransition::Connect).await
            .context("Failed to transition to Connecting state")?;
        
        // 根据平台过滤协议
        use crate::platform::{get_platform, Platform};
        let platform = get_platform();
        let original_count = protocols.len();
        let filtered_protocols: Vec<TransportProtocol> = protocols.into_iter()
            .filter(|p| {
                match platform {
                    Platform::Web => {
                        // Web 平台仅支持 WebSocket（虽然这里不会到达，因为 wasm32 有单独的实现）
                        matches!(p, TransportProtocol::WebSocket)
                    }
                    _ => {
                        // 其他平台支持所有协议
                        true
                    }
                }
            })
            .collect();
        
        if filtered_protocols.is_empty() {
            return Err(anyhow::anyhow!(
                "No supported protocols for platform {:?}",
                platform
            ));
        }
        
        // 如果过滤后的协议列表与原列表不同，记录日志
        if filtered_protocols.len() != original_count {
            tracing::info!(
                platform = ?platform,
                original_count,
                filtered_count = filtered_protocols.len(),
                "Filtered protocols based on platform capabilities"
            );
        }
        
        // 读取配置（一次性读取所有需要的配置）
        let config_guard = self.config.read().await;
        
        // 获取消息监听器（必须设置）
        let message_listener = self.message_listener.lock().await.clone()
            .ok_or_else(|| anyhow::anyhow!("MessageListener not set. Please set it before connecting."))?;
        
        // 构建客户端配置（使用过滤后的协议列表）
        let mut builder = Self::build_client_builder(&config_guard, filtered_protocols.clone(), message_listener)?;
        
        // 创建并添加 ConnectionEventObserver
        let connection_event_observer = Arc::new(ConnectionEventObserver {
            event_bus: Arc::clone(&self.event_bus),
            state_machine: Arc::clone(&self.state_machine),
            state: Arc::clone(&self.state), // 保持兼容性
            active_protocol: Arc::clone(&self.active_protocol),
        });
        builder = builder.with_observer(connection_event_observer);
        
        // 释放配置锁
        drop(config_guard);
        
        // ============================================================
        // 使用协议竞速连接（由 HybridClient::connect_with_race 处理）
        // ============================================================
        let client = builder.build_with_race().await?;
        
        // 获取连接成功的协议
        let active_protocol = client.active_protocol();
        *self.active_protocol.write().await = Some(active_protocol);
        
        // 使用状态机进行状态转换（会自动发布事件）
        self.set_state(StateTransition::Connected).await
            .context("Failed to transition to Connected state")?;
        
        // 发布连接事件（包含协议信息）
        self.event_bus.publish(Event::Connection(ConnectionEvent::Connected { protocol: Some(active_protocol) }));
        
        // 存储客户端
        *self.client.lock().await = Some(client);
        
        // ============================================================
        // 关键修复：等待认证完成（CONNECT_ACK）
        // ============================================================
        // ConnectionEventObserver 在 tokio::spawn 中异步处理 CONNECT_ACK
        // 我们需要等待状态更新为 Authenticated 后再返回
        // 这样可以确保 login 方法检查状态时，状态已经是 Authenticated
        use tokio::time::{Duration, sleep};
        use std::time::Instant;
        
        let auth_wait_start = Instant::now();
        let max_auth_wait = Duration::from_secs(8); // 增加等待时间到 8 秒，给状态更新更多时间
        
        // 先快速检查一次状态（可能状态已经更新）
        let initial_state = *self.state.read().await;
        if matches!(initial_state, ConnectionState::Authenticated) {
            tracing::info!("✅ Authentication already completed in connect_with_race (initial check)");
        } else {
            // 等待状态更新
            loop {
                let current_state = *self.state.read().await;
                
                // 如果状态已经是 Authenticated，认证成功
                if matches!(current_state, ConnectionState::Authenticated) {
                    tracing::info!("✅ Authentication completed in connect_with_race");
                    break;
                }
                
                // 检查是否超时
                if auth_wait_start.elapsed() > max_auth_wait {
                    // 超时，但连接仍然有效，记录警告但继续
                    tracing::warn!(
                        "Authentication timeout in connect_with_race after {} seconds, current state: {:?}. Continuing anyway.",
                        max_auth_wait.as_secs(),
                        current_state
                    );
                    // 注意：不返回错误，让 login 方法继续处理
                    // login 方法会再次检查状态并等待
                    break;
                }
                
                // 等待一小段时间后再次检查（增加检查频率）
                sleep(Duration::from_millis(100)).await;
            }
        }
        
        // 如果启用了自动重连，启动重连监听（使用过滤后的协议列表）
        let auto_reconnect = {
            let config = self.config.read().await;
            config.auto_reconnect
        };
        if auto_reconnect {
            self.start_reconnect_listener(filtered_protocols).await;
        }
        
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn connect_with_race(&self, _protocols: Vec<TransportProtocol>) -> anyhow::Result<()> {
        // 使用状态机进行状态转换
        self.set_state(StateTransition::Connected).await
            .context("Failed to transition to Connected state")?;
        self.event_bus.publish(Event::Connection(ConnectionEvent::Connected { protocol: None }));
        Ok(())
    }
    
    /// 连接到服务器（单协议模式）
    /// 
    /// 只使用指定的协议
    /// 
    /// 使用 FlareClientBuilder 构建客户端（参照 flare_chat_client.rs 示例）
    /// 注意：单协议模式也使用协议竞速，但只传入一个协议
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn connect(&self, protocol: TransportProtocol) -> anyhow::Result<()> {
        // 单协议模式实际上就是协议竞速模式，但只传入一个协议
        self.connect_with_race(vec![protocol]).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn connect(&self, _protocol: TransportProtocol) -> anyhow::Result<()> {
        // 使用状态机进行状态转换
        self.set_state(StateTransition::Connected).await
            .context("Failed to transition to Connected state")?;
        self.event_bus.publish(Event::Connection(ConnectionEvent::Connected { protocol: None }));
        Ok(())
    }
    
    /// 启动重连监听器
    /// 
    /// 监听断开事件，自动重连
    #[cfg(not(target_arch = "wasm32"))]
    async fn start_reconnect_listener(&self, protocols: Vec<TransportProtocol>) {
        let mut event_rx = self.event_bus.subscribe();
        let client = Arc::clone(&self.client);
        let config = Arc::clone(&self.config);
        let state = Arc::clone(&self.state);
        let active_protocol = Arc::clone(&self.active_protocol);
        let event_bus = Arc::clone(&self.event_bus);
        let reconnect_handle = Arc::clone(&self.reconnect_handle);
        let message_listener = Arc::clone(&self.message_listener);
        
        // 读取配置中的重连参数
        let (reconnect_interval, max_attempts) = {
            let config_guard = self.config.read().await;
            (
                Duration::from_secs(config_guard.reconnect_interval),
                config_guard.max_reconnect_attempts,
            )
        };
        
        let handle = tokio::spawn(async move {
            let mut reconnect_attempts = 0u32;
            
            while let Ok(event) = event_rx.recv().await {
                match event {
                    Event::Connection(ConnectionEvent::Disconnected) => {
                        // 检查是否应该重连
                        if max_attempts > 0 && reconnect_attempts >= max_attempts {
                            tracing::warn!("达到最大重连次数 {}，停止重连", max_attempts);
                            event_bus.publish(Event::Connection(
                                ConnectionEvent::Error("Max reconnect attempts reached".to_string())
                            ));
                            break;
                        }
                        
                        // 获取消息监听器
                        let listener = message_listener.lock().await.clone();
                        if listener.is_none() {
                            tracing::warn!(
                                "MessageListener not set, cannot reconnect"
                            );
                            continue;
                        }
                        let listener = listener.unwrap();
                        
                        // 更新状态为重连中
                        *state.write().await = ConnectionState::Reconnecting;
                        reconnect_attempts += 1;
                        
                        event_bus.publish(Event::Connection(ConnectionEvent::Reconnecting));
                        
                        tracing::info!(
                            attempt = reconnect_attempts,
                            "Starting reconnection"
                        );
                        
                        // 等待重连间隔
                        tokio::time::sleep(reconnect_interval).await;
                        
                        // 尝试重连
                        let protocols_clone = protocols.clone();
                        let reconnect_result = if protocols_clone.len() > 1 {
                            // 协议竞速模式
                            Self::reconnect_with_race_impl(
                                &client,
                                &config,
                                listener,
                                protocols_clone,
                                &state,
                                &active_protocol,
                                &event_bus,
                            ).await
                        } else {
                            // 单协议模式
                            Self::reconnect_single_impl(
                                &client,
                                &config,
                                listener,
                                protocols_clone[0],
                                &state,
                                &active_protocol,
                                &event_bus,
                            ).await
                        };
                        
                        match reconnect_result {
                            Ok(()) => {
                                tracing::info!("重连成功");
                                reconnect_attempts = 0; // 重置重连计数
                                event_bus.publish(Event::Connection(ConnectionEvent::Reconnected));
                            }
                            Err(e) => {
                                tracing::warn!("重连失败: {}", e);
                                // 继续循环，等待下次断开事件
                            }
                        }
                    }
                    Event::Connection(ConnectionEvent::Connected { .. }) => {
                        // 连接成功，重置重连计数
                        reconnect_attempts = 0;
                    }
                    _ => {}
                }
            }
        });
        
        *reconnect_handle.lock().await = Some(handle);
    }
    
    /// 重连（协议竞速模式）
    /// 
    /// 使用 FlareClientBuilder 构建客户端（参照 flare_chat_client.rs 示例）
    #[cfg(not(target_arch = "wasm32"))]
    async fn reconnect_with_race_impl(
        client: &Arc<Mutex<Option<FlareClient>>>,
        config: &Arc<tokio::sync::RwLock<ClientConfig>>,
        message_listener: Arc<SDKMessageListener>,
        protocols: Vec<TransportProtocol>,
        state: &Arc<RwLock<ConnectionState>>,
        active_protocol: &Arc<RwLock<Option<TransportProtocol>>>,
        event_bus: &Arc<EventBus>,
    ) -> anyhow::Result<()> {
        // 读取配置
        let config_guard = config.read().await;
        
        // 使用 FlareClientBuilder 构建客户端（参照 connect_with_race 的实现）
        let mut builder = FlareClientBuilder::new(config_guard.server_url.clone());
        
        // 设置消息监听器
        builder = builder.with_listener(message_listener);
        
        // 添加中间件（参照 connect_with_race）
        use flare_core::common::message::{
            ArcMessageMiddleware,
            LoggingMiddleware, MetricsMiddleware, LogLevel,
        };
        let logging_middleware = Arc::new(LoggingMiddleware::new("SDKClientLogging")
            .with_level(LogLevel::Info)) as ArcMessageMiddleware;
        builder = builder.with_middleware(logging_middleware);
        
        let metrics_middleware = Arc::new(MetricsMiddleware::new("SDKClientMetrics")) as ArcMessageMiddleware;
        builder = builder.with_middleware(metrics_middleware);
        
        // 设置协议竞速
        builder = builder.with_protocol_race(protocols.clone());
        
        // 设置协议地址
        if let Some(ref protocol_urls) = config_guard.protocol_urls {
            for (protocol, url) in protocol_urls {
                builder = builder.with_protocol_url(*protocol, url.clone());
            }
        }
        
        // 设置用户 ID
        if !config_guard.user_id.is_empty() {
            builder = builder.with_user_id(config_guard.user_id.clone());
        }
        
        // 设置设备信息（参照 connect_with_race 的完整方式）
        use flare_core::common::device::{DeviceInfo as FlareDeviceInfo, DevicePlatform as FlareDevicePlatform};
        let platform = match config_guard.device_platform {
            crate::config::DevicePlatform::Web => FlareDevicePlatform::Web,
            crate::config::DevicePlatform::Android => FlareDevicePlatform::Android,
            crate::config::DevicePlatform::IOS => FlareDevicePlatform::IOS,
            crate::config::DevicePlatform::HarmonyOS => FlareDevicePlatform::HarmonyOS,
            crate::config::DevicePlatform::Desktop => FlareDevicePlatform::PC,
        };
        
        let mut device_info = FlareDeviceInfo::new(
            config_guard.device_id.clone(),
            platform.clone(),
        );
        device_info = device_info.with_model(platform.as_str().to_string());
        
        if let Some(ref app_version) = config_guard.app_version {
            device_info = device_info.with_app_version(app_version.clone());
        } else {
            device_info = device_info.with_app_version("1.0.0".to_string());
        }
        
        let system_version = match &platform {
            FlareDevicePlatform::PC => "macOS/Linux/Windows".to_string(),
            FlareDevicePlatform::Android => "Android".to_string(),
            FlareDevicePlatform::IOS => "iOS".to_string(),
            FlareDevicePlatform::Web => "Web Browser".to_string(),
            FlareDevicePlatform::H5 => "Mobile Browser".to_string(),
            FlareDevicePlatform::HarmonyOS => "HarmonyOS".to_string(),
            FlareDevicePlatform::Other(_) => "Unknown".to_string(),
        };
        device_info = device_info.with_system_version(system_version);
        
        builder = builder.with_device_info(device_info);
        
        // 设置 token（如果提供）
        if let Some(ref token) = config_guard.token {
            builder = builder.with_token(token.clone());
        }
        
        // 设置心跳配置（使用 heartbeat_interval）
        use flare_core::common::config_types::HeartbeatConfig;
        let heartbeat_config = HeartbeatConfig::default()
            .with_interval(std::time::Duration::from_secs(config_guard.heartbeat_interval))
            .with_timeout(std::time::Duration::from_secs(config_guard.heartbeat_interval * 3));
        builder = builder.with_heartbeat(heartbeat_config);
        
        // 设置连接超时
        builder = builder.with_connect_timeout(
            std::time::Duration::from_secs(config_guard.connect_timeout)
        );
        
        // 设置协议竞速超时（必须设置，否则使用默认 5 秒，可能导致超时）
        // 增加超时时间，给连接更多时间稳定
        let race_timeout = config_guard.race_timeout
            .unwrap_or(std::time::Duration::from_secs(config_guard.connect_timeout.max(20)));
        builder = builder.with_race_timeout(race_timeout);
        
        // 设置重连配置
        builder = builder.with_reconnect_interval(
            std::time::Duration::from_secs(config_guard.reconnect_interval)
        );
        
        if config_guard.max_reconnect_attempts > 0 {
            builder = builder.with_max_reconnect_attempts(Some(config_guard.max_reconnect_attempts));
        }
        
        drop(config_guard);
        
        // 使用协议竞速连接
        let new_client = builder.build_with_race().await?;
        let protocol = new_client.active_protocol();
        
        *active_protocol.write().await = Some(protocol);
        *client.lock().await = Some(new_client);
        
        // 等待一小段时间，确保连接稳定
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        // 再次检查连接是否仍然有效
        // 注意：这里不能直接检查 new_client，需要通过状态判断
        // 如果连接立即断开，状态会被更新为 Disconnected
        
        let current_state = *state.read().await;
        if current_state == ConnectionState::Disconnected {
            return Err(anyhow::anyhow!("Connection lost immediately after establishment. This may indicate server-side issues or authentication problems."));
        }
        
        *state.write().await = ConnectionState::Connected;
        event_bus.publish(Event::Connection(ConnectionEvent::Connected { protocol: Some(protocol) }));
        
        Ok(())
    }
    
    /// 重连（单协议模式）
    /// 
    /// 使用 FlareClientBuilder 构建客户端（参照 flare_chat_client.rs 示例）
    /// 单协议模式实际上就是协议竞速模式，但只传入一个协议
    #[cfg(not(target_arch = "wasm32"))]
    async fn reconnect_single_impl(
        client: &Arc<Mutex<Option<FlareClient>>>,
        config: &Arc<tokio::sync::RwLock<ClientConfig>>,
        message_listener: Arc<SDKMessageListener>,
        protocol: TransportProtocol,
        state: &Arc<RwLock<ConnectionState>>,
        active_protocol: &Arc<RwLock<Option<TransportProtocol>>>,
        event_bus: &Arc<EventBus>,
    ) -> anyhow::Result<()> {
        // 单协议模式实际上就是协议竞速模式，但只传入一个协议
        Self::reconnect_with_race_impl(
            client,
            config,
            message_listener,
            vec![protocol],
            state,
            active_protocol,
            event_bus,
        ).await
    }
    
    /// 断开连接
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(handle) = self.reconnect_handle.lock().await.take() { handle.abort(); }
            // 使用状态机进行状态转换（会自动发布事件）
            let _ = self.set_state(StateTransition::Disconnect).await;
            *self.active_protocol.write().await = None;
            return Ok(());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(handle) = self.reconnect_handle.lock().await.take() { handle.abort(); }
            if let Some(client) = self.client.lock().await.take() {
                let _ = client.disconnect().await;
            }
            // 使用状态机进行状态转换（会自动发布事件）
            let _ = self.set_state(StateTransition::Disconnect).await;
            *self.active_protocol.write().await = None;
            Ok(())
        }
    }
    
    /// 发送 Frame
    pub async fn send_frame(&self, frame: &flare_core::common::protocol::Frame) -> anyhow::Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            return Err(anyhow::anyhow!("send_frame not supported on wasm stub connection"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // 记录关键字段，便于排查请求路径
            let cmd_name = frame.command.as_ref()
                .and_then(|c| c.r#type.as_ref())
                .map(|t| match t { 
                    flare_core::common::protocol::flare::core::commands::command::Type::Message(_) => "Message",
                    flare_core::common::protocol::flare::core::commands::command::Type::System(_) => "System",
                    flare_core::common::protocol::flare::core::commands::command::Type::Custom(custom) => custom.name.as_str(),
                    flare_core::common::protocol::flare::core::commands::command::Type::Notification(_) => "Notification",
                })
                .unwrap_or("<none>");
            let meta_keys: Vec<&str> = frame.metadata.keys().map(|k| k.as_str()).collect();
            tracing::debug!(msg_id = %frame.message_id, ts = frame.timestamp, cmd = %cmd_name, meta_keys = ?meta_keys, "Sending frame");
            let mut client_guard = self.client.lock().await;
            let client = client_guard.as_mut()
                .ok_or_else(|| anyhow::anyhow!("Not connected"))?;
            client.send_frame(frame).await?;
            Ok(())
        }
    }
    
    /// 获取连接状态
    /// 获取当前连接状态（通过状态机）
    pub async fn state(&self) -> ConnectionState {
        self.state_machine.current_state().await
    }
    
    /// 设置连接状态（内部方法，使用状态机）
    /// 
    /// 使用状态机确保状态转换的有效性
    pub(crate) async fn set_state(&self, transition: StateTransition) -> anyhow::Result<ConnectionState> {
        let new_state = self.state_machine.transition(transition).await?;
        // 同步到旧的状态字段（保持兼容性）
        *self.state.write().await = new_state;
        Ok(new_state)
    }
    
    /// 强制设置状态（不验证，用于恢复或特殊情况）
    /// 
    /// ⚠️ 警告：此方法会跳过状态验证，只在特殊情况下使用
    pub(crate) async fn force_set_state(&self, new_state: ConnectionState) {
        self.state_machine.force_set_state(new_state).await;
        *self.state.write().await = new_state;
    }
    
    /// 获取当前使用的协议
    pub async fn active_protocol(&self) -> Option<TransportProtocol> {
        *self.active_protocol.read().await
    }
    
    /// 检查是否已连接
    pub async fn is_connected(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        { return *self.state.read().await == ConnectionState::Connected; }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let client_guard = self.client.lock().await;
            if let Some(ref client) = *client_guard { client.is_connected() } else { false }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_initial_state_disconnected() {
        let cfg = ClientConfig::builder()
            .server_url("wss://example.com")
            .user_id("u1")
            .device_id("d1")
            .build()
            .unwrap();
        let bus = Arc::new(EventBus::new());
        let mgr = ConnectionManager::new(Arc::new(tokio::sync::RwLock::new(cfg)), Arc::clone(&bus));
        assert_eq!(mgr.state().await, ConnectionState::Disconnected);
    }
}

impl ConnectionManager {
    /// 构建 FlareClientBuilder
    /// 
    /// 提取配置构建逻辑，包括：
    /// - 中间件配置
    /// - 协议配置
    /// - 设备信息配置
    /// - 用户和 Token 配置
    /// - 连接配置（心跳、超时、重连）
    #[cfg(not(target_arch = "wasm32"))]
    fn build_client_builder(
        config: &ClientConfig,
        protocols: Vec<TransportProtocol>,
        message_listener: Arc<SDKMessageListener>,
    ) -> anyhow::Result<FlareClientBuilder> {
        use flare_core::common::message::{
            ArcMessageMiddleware,
            LoggingMiddleware, MetricsMiddleware, LogLevel,
        };
        use flare_core::common::config_types::HeartbeatConfig;

        let mut builder = FlareClientBuilder::new(config.server_url.clone());
        
        // 设置消息监听器
        builder = builder.with_listener(message_listener);
        
        // 添加中间件
        let logging_middleware = Arc::new(LoggingMiddleware::new("SDKClientLogging")
            .with_level(LogLevel::Info)) as ArcMessageMiddleware;
        builder = builder.with_middleware(logging_middleware);
        
        let metrics_middleware = Arc::new(MetricsMiddleware::new("SDKClientMetrics")) as ArcMessageMiddleware;
        builder = builder.with_middleware(metrics_middleware);
        
        // 协议配置
        builder = builder.with_protocol_race(protocols.clone());
        
        if let Some(ref protocol_urls) = config.protocol_urls {
            for (protocol, url) in protocol_urls {
                builder = builder.with_protocol_url(*protocol, url.clone());
            }
        }
        
        // 设备信息配置
        let device_info = Self::build_device_info(config)?;
        builder = builder.with_device_info(device_info);
        
        // 用户 ID 和 Token 配置
        if !config.user_id.is_empty() {
            builder = builder.with_user_id(config.user_id.clone());
        }
        
        if let Some(ref token) = config.token {
            builder = builder.with_token(token.clone());
        }
        
        // 连接配置
        let heartbeat_config = HeartbeatConfig::default()
            .with_interval(std::time::Duration::from_secs(config.heartbeat_interval))
            .with_timeout(std::time::Duration::from_secs(config.heartbeat_interval * 3));
        builder = builder.with_heartbeat(heartbeat_config);
        
        builder = builder.with_connect_timeout(
            std::time::Duration::from_secs(config.connect_timeout)
        );
        
        let race_timeout = config.race_timeout
            .unwrap_or(std::time::Duration::from_secs(config.connect_timeout));
        builder = builder.with_race_timeout(race_timeout);
        
        builder = builder.with_reconnect_interval(
            std::time::Duration::from_secs(config.reconnect_interval)
        );
        
        if config.max_reconnect_attempts > 0 {
            builder = builder.with_max_reconnect_attempts(Some(config.max_reconnect_attempts));
        }
        
        Ok(builder)
    }

    /// 构建设备信息
    /// 
    /// 从配置中构建完整的设备信息，包括平台、型号、版本等
    #[cfg(not(target_arch = "wasm32"))]
    fn build_device_info(config: &ClientConfig) -> anyhow::Result<flare_core::common::device::DeviceInfo> {
        use flare_core::common::device::{DeviceInfo as FlareDeviceInfo, DevicePlatform as FlareDevicePlatform};
        
        let platform = match config.device_platform {
            crate::config::DevicePlatform::Web => FlareDevicePlatform::Web,
            crate::config::DevicePlatform::Android => FlareDevicePlatform::Android,
            crate::config::DevicePlatform::IOS => FlareDevicePlatform::IOS,
            crate::config::DevicePlatform::HarmonyOS => FlareDevicePlatform::HarmonyOS,
            crate::config::DevicePlatform::Desktop => FlareDevicePlatform::PC,
        };
        
        let mut device_info = FlareDeviceInfo::new(
            config.device_id.clone(),
            platform.clone(),
        );
        
        // 设置 model（使用平台名称作为标识）
        device_info = device_info.with_model(platform.as_str().to_string());
        
        // 设置应用版本
        let app_version = config.app_version.clone().unwrap_or_else(|| "1.0.0".to_string());
        device_info = device_info.with_app_version(app_version);
        
        // 设置系统版本（仅用于记录，不作为平台判定标准）
        let system_version = match &platform {
            FlareDevicePlatform::PC => "macOS/Linux/Windows".to_string(),
            FlareDevicePlatform::Android => "Android".to_string(),
            FlareDevicePlatform::IOS => "iOS".to_string(),
            FlareDevicePlatform::Web => "Web Browser".to_string(),
            FlareDevicePlatform::H5 => "Mobile Browser".to_string(),
            FlareDevicePlatform::HarmonyOS => "HarmonyOS".to_string(),
            FlareDevicePlatform::Other(_) => "Unknown".to_string(),
        };
        device_info = device_info.with_system_version(system_version);
        
        Ok(device_info)
    }
}
