//! 连接管理器
//!
//! 负责管理连接生命周期：连接、断开、重连等

use crate::infrastructure::connection::client_builder::ClientBuilder;
use crate::infrastructure::connection::event_observer::ConnectionEventObserver;
use crate::infrastructure::connection::message_listener::SDKMessageListener;
use crate::infrastructure::connection::state_machine::{
    ConnectionStateMachine, StateMachineConfig, StateTransition,
};
use crate::infrastructure::event::{ConnectionEvent, Event, EventBus};
use crate::shared::config::ClientConfig;
use anyhow::Context;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::client::builder::flare::FlareClient;
use flare_core::common::config_types::TransportProtocol;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
    active_protocol: Arc<RwLock<Option<TransportProtocol>>>,
    event_bus: Arc<EventBus>,
    reconnect_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// 消息监听器（用于 FlareClientBuilder）
    message_listener: Arc<Mutex<Option<Arc<SDKMessageListener>>>>,
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

        Self {
            #[cfg(not(target_arch = "wasm32"))]
            client: Arc::new(Mutex::new(None)),
            config,
            state_machine,
            active_protocol: Arc::new(RwLock::new(None)),
            event_bus,
            reconnect_handle: Arc::new(Mutex::new(None)),
            message_listener: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置消息监听器（异步方法）
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
        self.set_state(StateTransition::Connect)
            .await
            .context("Failed to transition to Connecting state")?;

        // 根据平台过滤协议
        use crate::shared::platform::{Platform, get_platform};
        let platform = get_platform();
        let original_count = protocols.len();
        let filtered_protocols: Vec<TransportProtocol> = protocols
            .into_iter()
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
        let server_url = {
            let config_guard = self.config.read().await;
            config_guard.server_url.clone()
        };

        // 获取消息监听器（必须设置）
        let message_listener = self.message_listener.lock().await.clone().ok_or_else(|| {
            anyhow::anyhow!("MessageListener not set. Please set it before connecting.")
        })?;

        // 构建客户端配置（使用过滤后的协议列表）
        let config_guard = self.config.read().await;
        let mut builder =
            ClientBuilder::build(&config_guard, filtered_protocols.clone(), message_listener)?;

        // 创建并添加 ConnectionEventObserver
        let connection_event_observer = Arc::new(ConnectionEventObserver::new(
            Arc::clone(&self.event_bus),
            Arc::clone(&self.state_machine),
            Arc::clone(&self.active_protocol),
        ));
        builder = builder.with_observer(connection_event_observer);

        // 释放配置锁
        drop(config_guard);

        // ============================================================
        // 使用协议竞速连接（由 HybridClient::connect_with_race 处理）
        // ============================================================
        tracing::info!("开始连接服务器: {}", server_url);
        tracing::info!("协议列表: {:?}", filtered_protocols);

        let client = builder.build_with_race().await.map_err(|e| {
            let error_msg = format!("{}", e);
            let error_debug = format!("{:?}", e);
            let error_chain = format!("{:?}", e);
            tracing::error!("连接失败: {}", error_msg);
            tracing::error!("连接失败详情 (Display): {}", error_debug);
            tracing::error!("连接失败详情 (Debug): {}", error_chain);
            tracing::error!("服务器地址: {}", server_url);
            tracing::error!("协议列表: {:?}", filtered_protocols);
            eprintln!("[ERROR] 连接失败: {}", error_msg);
            eprintln!("[ERROR] 连接失败详情: {}", error_debug);
            eprintln!("[ERROR] 服务器地址: {}", server_url);
            eprintln!("[ERROR] 协议列表: {:?}", filtered_protocols);
            anyhow::anyhow!("连接失败: {} (服务器: {})", error_msg, server_url)
                .context("Failed to connect to server")
        })?;

        // 获取连接成功的协议
        let active_protocol = client.active_protocol();
        *self.active_protocol.write().await = Some(active_protocol);

        // 使用状态机进行状态转换（会自动发布事件）
        self.set_state(StateTransition::Connected)
            .await
            .context("Failed to transition to Connected state")?;

        // 发布连接事件（包含协议信息）
        self.event_bus
            .publish(Event::Connection(ConnectionEvent::Connected {
                protocol: Some(active_protocol),
            }));

        // 存储客户端
        *self.client.lock().await = Some(client);

        // ============================================================
        // 关键修复：等待认证完成（CONNECT_ACK）
        // ============================================================
        // ConnectionEventObserver 在 tokio::spawn 中异步处理 CONNECT_ACK
        // 我们需要等待状态更新为 Authenticated 后再返回
        // 这样可以确保 login 方法检查状态时，状态已经是 Authenticated
        use std::time::Instant;
        use tokio::time::{Duration, sleep};

        let auth_wait_start = Instant::now();
        let max_auth_wait = Duration::from_secs(8); // 增加等待时间到 8 秒，给状态更新更多时间

        // 先快速检查一次状态（可能状态已经更新）
        let initial_state = self.state_machine.current_state().await;
        if matches!(initial_state, ConnectionState::Authenticated) {
            tracing::info!(
                "✅ Authentication already completed in connect_with_race (initial check)"
            );
        } else {
            // 等待状态更新
            loop {
                let current_state = self.state_machine.current_state().await;

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
    pub async fn connect_with_race(
        &self,
        _protocols: Vec<TransportProtocol>,
    ) -> anyhow::Result<()> {
        // 使用状态机进行状态转换
        self.set_state(StateTransition::Connected)
            .await
            .context("Failed to transition to Connected state")?;
        self.event_bus
            .publish(Event::Connection(ConnectionEvent::Connected {
                protocol: None,
            }));
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
        self.set_state(StateTransition::Connected)
            .await
            .context("Failed to transition to Connected state")?;
        self.event_bus
            .publish(Event::Connection(ConnectionEvent::Connected {
                protocol: None,
            }));
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
        let state_machine = Arc::clone(&self.state_machine);
        let active_protocol = Arc::clone(&self.active_protocol);
        let event_bus = Arc::clone(&self.event_bus);
        let reconnect_handle = Arc::clone(&self.reconnect_handle);
        let message_listener = Arc::clone(&self.message_listener);

        // 读取配置中的重连参数
        let max_attempts = {
            let config_guard = self.config.read().await;
            config_guard.max_reconnect_attempts
        };

        let handle = tokio::spawn(async move {
            let mut reconnect_attempts = 0u32;

            while let Ok(event) = event_rx.recv().await {
                match event {
                    Event::Connection(ConnectionEvent::Kicked { reason }) => {
                        // 被踢下线（设备冲突等），不应该自动重连
                        tracing::warn!(
                            reason = %reason,
                            "Connection kicked, stopping reconnection attempts"
                        );
                        event_bus.publish(Event::Connection(ConnectionEvent::Error(format!(
                            "Connection kicked: {}",
                            reason
                        ))));
                        break; // 停止重连循环
                    }
                    Event::Connection(ConnectionEvent::Disconnected) => {
                        // 检查是否应该重连
                        if max_attempts > 0 && reconnect_attempts >= max_attempts {
                            tracing::warn!("达到最大重连次数 {}，停止重连", max_attempts);
                            event_bus.publish(Event::Connection(ConnectionEvent::Error(
                                "Max reconnect attempts reached".to_string(),
                            )));
                            break;
                        }

                        // 获取消息监听器
                        let listener = message_listener.lock().await.clone();
                        if listener.is_none() {
                            tracing::warn!("MessageListener not set, cannot reconnect");
                            continue;
                        }
                        let listener = listener.unwrap();

                        // 更新状态为重连中
                        let current_state = state_machine.current_state().await;
                        if current_state != ConnectionState::Reconnecting {
                            if let Err(e) =
                                state_machine.transition(StateTransition::Reconnect).await
                            {
                                tracing::warn!(error = %e, "Failed to transition to Reconnecting state");
                                continue; // 如果状态转换失败，跳过本次重连
                            }
                        }
                        reconnect_attempts += 1;

                        event_bus.publish(Event::Connection(ConnectionEvent::Reconnecting));

                        tracing::info!(attempt = reconnect_attempts, "Starting reconnection");

                        // 使用智能重连策略（参考 Telegram 设计）
                        let strategy = state_machine.reconnect_strategy();
                        strategy.record_attempt().await;
                        let delay = strategy.next_delay().await;
                        tokio::time::sleep(delay).await;

                        // 重连前，确保状态机从 Reconnecting 转换到 Connecting
                        if let Err(e) = state_machine.transition(StateTransition::Connect).await {
                            tracing::warn!(error = %e, "Failed to transition from Reconnecting to Connecting state");
                            continue;
                        }

                        // 尝试重连
                        let protocols_clone = protocols.clone();
                        let reconnect_result = if protocols_clone.len() > 1 {
                            // 协议竞速模式
                            Self::reconnect_with_race_impl(
                                &client,
                                &config,
                                listener,
                                protocols_clone,
                                &state_machine,
                                &active_protocol,
                                &event_bus,
                            )
                            .await
                        } else {
                            // 单协议模式
                            Self::reconnect_single_impl(
                                &client,
                                &config,
                                listener,
                                protocols_clone[0],
                                &state_machine,
                                &active_protocol,
                                &event_bus,
                            )
                            .await
                        };

                        match reconnect_result {
                            Ok(()) => {
                                tracing::info!("重连成功");
                                reconnect_attempts = 0; // 重置重连计数

                                // 重置重连策略（连接成功后）
                                let strategy = state_machine.reconnect_strategy();
                                strategy.reset().await;

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
        state_machine: &Arc<ConnectionStateMachine>,
        active_protocol: &Arc<RwLock<Option<TransportProtocol>>>,
        event_bus: &Arc<EventBus>,
    ) -> anyhow::Result<()> {
        // 读取配置
        let config_guard = config.read().await;

        // 使用 ClientBuilder 构建客户端
        let builder = ClientBuilder::build(&config_guard, protocols.clone(), message_listener)?;

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

        let current_state = state_machine.current_state().await;
        if current_state == ConnectionState::Disconnected {
            return Err(anyhow::anyhow!(
                "Connection lost immediately after establishment. This may indicate server-side issues or authentication problems."
            ));
        }

        if let Err(e) = state_machine.transition(StateTransition::Connected).await {
            tracing::warn!(error = %e, "Failed to transition to Connected state during reconnect");
        }
        event_bus.publish(Event::Connection(ConnectionEvent::Connected {
            protocol: Some(protocol),
        }));

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
        state_machine: &Arc<ConnectionStateMachine>,
        active_protocol: &Arc<RwLock<Option<TransportProtocol>>>,
        event_bus: &Arc<EventBus>,
    ) -> anyhow::Result<()> {
        // 单协议模式实际上就是协议竞速模式，但只传入一个协议
        Self::reconnect_with_race_impl(
            client,
            config,
            message_listener,
            vec![protocol],
            state_machine,
            active_protocol,
            event_bus,
        )
        .await
    }

    /// 断开连接
    pub async fn disconnect(&self) -> anyhow::Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(handle) = self.reconnect_handle.lock().await.take() {
                handle.abort();
            }
            // 使用状态机进行状态转换（会自动发布事件）
            let _ = self.set_state(StateTransition::Disconnect).await;
            *self.active_protocol.write().await = None;
            return Ok(());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(handle) = self.reconnect_handle.lock().await.take() {
                handle.abort();
            }
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
    pub async fn send_frame(
        &self,
        frame: &flare_core::common::protocol::Frame,
    ) -> anyhow::Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            return Err(anyhow::anyhow!(
                "send_frame not supported on wasm stub connection"
            ));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            // 记录关键字段，便于排查请求路径
            let cmd_name = frame.command.as_ref()
                .and_then(|c| c.r#type.as_ref())
                .map(|t| match t {                    flare_core::common::protocol::flare::core::commands::command::Type::Message(_) => "Message",
                    flare_core::common::protocol::flare::core::commands::command::Type::System(_) => "System",
                    flare_core::common::protocol::flare::core::commands::command::Type::Custom(custom) => custom.name.as_str(),
                    flare_core::common::protocol::flare::core::commands::command::Type::Notification(_) => "Notification",
                })
                .unwrap_or("<none>");
            let meta_keys: Vec<&str> = frame.metadata.keys().map(|k| k.as_str()).collect();
            tracing::debug!(msg_id = %frame.message_id, ts = frame.timestamp, cmd = %cmd_name, meta_keys = ?meta_keys, "Sending frame");
            let mut client_guard = self.client.lock().await;
            let client = client_guard
                .as_mut()
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
    pub(crate) async fn set_state(
        &self,
        transition: StateTransition,
    ) -> anyhow::Result<ConnectionState> {
        self.state_machine.transition(transition).await
    }

    /// 强制设置状态（不验证，用于恢复或特殊情况）
    ///
    /// ⚠️ 警告：此方法会跳过状态验证，只在特殊情况下使用
    #[allow(dead_code)] // 保留用于特殊情况下的状态恢复
    pub(crate) async fn force_set_state(&self, new_state: ConnectionState) {
        self.state_machine.force_set_state(new_state).await;
    }

    /// 获取当前使用的协议
    pub async fn active_protocol(&self) -> Option<TransportProtocol> {
        *self.active_protocol.read().await
    }

    /// 检查是否已连接
    pub async fn is_connected(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            matches!(
                self.state_machine.current_state().await,
                ConnectionState::Connected | ConnectionState::Authenticated
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let client_guard = self.client.lock().await;
            if let Some(ref client) = *client_guard {
                client.is_connected()
            } else {
                matches!(
                    self.state_machine.current_state().await,
                    ConnectionState::Connected | ConnectionState::Authenticated
                )
            }
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
