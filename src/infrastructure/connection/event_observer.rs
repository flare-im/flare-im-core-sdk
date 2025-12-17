//! 连接事件观察者
//!
//! 将 flare-core 的连接事件转换为 SDK 的事件

use crate::infrastructure::connection::manager::ConnectionState;
use crate::infrastructure::connection::state_machine::{ConnectionStateMachine, StateTransition};
use crate::infrastructure::event::{ConnectionEvent, Event, EventBus};
use flare_core::common::config_types::TransportProtocol;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::transport::events::{ConnectionEvent as FlareConnectionEvent, ConnectionObserver};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 连接事件观察者
///
/// 将 flare-core 的连接事件转换为 SDK 的事件
#[cfg(not(target_arch = "wasm32"))]
pub struct ConnectionEventObserver {
    event_bus: Arc<EventBus>,
    state_machine: Arc<ConnectionStateMachine>,
    active_protocol: Arc<RwLock<Option<TransportProtocol>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ConnectionEventObserver {
    pub fn new(
        event_bus: Arc<EventBus>,
        state_machine: Arc<ConnectionStateMachine>,
        active_protocol: Arc<RwLock<Option<TransportProtocol>>>,
    ) -> Self {
        Self {
            event_bus,
            state_machine,
            active_protocol,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ConnectionObserver for ConnectionEventObserver {
    fn on_event(&self, event: &FlareConnectionEvent) {
        // 使用异步任务处理事件，避免阻塞
        let event_clone = event.clone();
        let state_machine = Arc::clone(&self.state_machine);
        let active_protocol = Arc::clone(&self.active_protocol);
        let event_bus = Arc::clone(&self.event_bus);

        tokio::spawn(async move {
            match &event_clone {
                FlareConnectionEvent::Connected => {
                    let current_state = state_machine.current_state().await;
                    if current_state != ConnectionState::Authenticated {
                        if let Err(e) = state_machine.transition(StateTransition::Connected).await {
                            tracing::warn!(error = %e, "Failed to transition to Connected state");
                        }
                    }
                    tracing::debug!("Connection established, waiting for authentication");
                }
                FlareConnectionEvent::Disconnected(reason) => {
                    let previous_state = state_machine.current_state().await;
                    if let Err(e) = state_machine.transition(StateTransition::Disconnect).await {
                        tracing::warn!(error = %e, "Failed to transition to Disconnected state");
                    }
                    *active_protocol.write().await = None;

                    if matches!(
                        previous_state,
                        ConnectionState::Connected | ConnectionState::Authenticating
                    ) {
                        tracing::warn!(
                            reason = %reason,
                            previous_state = ?previous_state,
                            "连接建立后立即断开，可能是认证失败或服务器问题"
                        );
                    }

                    tracing::info!("连接已断开: {}", reason);
                }
                FlareConnectionEvent::Message(data) => {
                    Self::handle_message(data, &state_machine, &event_bus).await;
                }
                FlareConnectionEvent::Error(err) => {
                    let event_bus_clone = Arc::clone(&event_bus);
                    let err_str = err.to_string();
                    tokio::spawn(async move {
                        event_bus_clone.publish(Event::Connection(ConnectionEvent::Error(err_str)));
                    });
                    tracing::error!(error = %err, "Connection error");
                }
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ConnectionEventObserver {
    async fn handle_message(
        data: &[u8],
        state_machine: &Arc<ConnectionStateMachine>,
        event_bus: &Arc<EventBus>,
    ) {
        use flare_core::common::MessageParser;
        use flare_core::common::compression::CompressionAlgorithm;
        use flare_core::common::protocol::SerializationFormat;
        use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemCommandType;

        tracing::debug!("ConnectionEventObserver received Message event, parsing frame...");

        let frame_result =
            MessageParser::new(SerializationFormat::Protobuf, CompressionAlgorithm::None)
                .parse(data);

        if let Ok(frame) = frame_result {
            tracing::debug!("Frame parsed successfully, checking for CONNECT_ACK...");

            // 检查是否是 CONNECT_ACK，如果是，立即更新状态为 Authenticated
            if let Some(cmd) = frame.command.as_ref() {
                if let Some(
                    flare_core::common::protocol::flare::core::commands::command::Type::System(
                        sys_cmd,
                    ),
                ) = cmd.r#type.as_ref()
                {
                    if sys_cmd.r#type == SystemCommandType::ConnectAck as i32 {
                        tracing::info!(
                            "CONNECT_ACK detected in ConnectionEventObserver, updating state..."
                        );

                        let current_state = state_machine.current_state().await;
                        if current_state != ConnectionState::Authenticated {
                            // 先转换到 Authenticating（如果需要）
                            if current_state == ConnectionState::Connected {
                                if let Err(e) = state_machine
                                    .transition(StateTransition::Authenticate)
                                    .await
                                {
                                    tracing::warn!(error = %e, "Failed to transition to Authenticating state");
                                }
                            }
                            // 然后转换到 Authenticated
                            if let Err(e) = state_machine
                                .transition(StateTransition::Authenticated)
                                .await
                            {
                                tracing::warn!(error = %e, "Failed to transition to Authenticated state");
                            } else {
                                tracing::info!(
                                    "Connection state updated to Authenticated (CONNECT_ACK received in ConnectionEventObserver)"
                                );

                                // 立即发布 Authenticated 事件（状态机已发布，但这里也发布以确保）
                                event_bus
                                    .publish(Event::Connection(ConnectionEvent::Authenticated));
                                tracing::info!(
                                    "Authenticated event published from ConnectionEventObserver"
                                );
                            }
                        } else {
                            tracing::debug!("Connection state already Authenticated");
                        }
                    } else {
                        tracing::debug!(
                            "System command is not CONNECT_ACK, type: {}",
                            sys_cmd.r#type
                        );
                    }
                } else {
                    tracing::debug!("Command is not a System command");
                }
            } else {
                tracing::debug!("Frame has no command");
            }

            // 发布 Frame 事件（异步处理，不阻塞当前任务）
            let event_bus_clone = Arc::clone(event_bus);
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
                let meta_keys: Vec<&str> =
                    frame_clone.metadata.keys().map(|k| k.as_str()).collect();
                tracing::debug!(msg_id = %frame_clone.message_id, cmd = %cmd_name, meta_keys = ?meta_keys, "Received frame");
                event_bus_clone.publish(Event::Connection(ConnectionEvent::FrameReceived(
                    frame_clone,
                )));
            });
        } else {
            tracing::warn!(
                error = ?frame_result.err(),
                "Failed to parse message frame"
            );
        }
    }
}
