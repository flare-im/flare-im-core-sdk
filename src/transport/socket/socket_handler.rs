use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use flare_core::client::builder::flare::MessageListener;
use flare_core::common::error::Result as FlareResult;
use flare_core::common::protocol::Frame;
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemType;
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::core::Dispatcher;
use crate::event::SdkEvent;
use crate::protocol::codec::Codec;

/// Socket 推送处理器 — 实现 flare-core `MessageListener`
///
/// 将 flare-core 收到的 Frame 解码为 ServerPacket 并交给 Dispatcher 分发。
/// 无论底层走的是 WebSocket 还是 QUIC，此处理器行为完全一致。
///
/// 内部持有 `ready_notify`：收到 CONNACK（`ConnectAck` 系统命令）后发出信号，
/// `SocketTransport::connect()` 会在此信号到达后才返回，
/// 以确保服务端已完成认证与连接上下文注册。
///
/// **注意**: PING/PONG 等心跳帧不触发 ready 信号——仅 `ConnectAck`、
/// `Error`、`Kicked` 系统命令或数据消息帧才代表认证流程已结束。
pub struct SocketHandler {
    dispatcher: Arc<Dispatcher>,
    codec: Arc<dyn Codec>,
    ready_notify: Arc<Notify>,
    notified: AtomicBool,
}

impl SocketHandler {
    pub fn new(
        dispatcher: Arc<Dispatcher>,
        codec: Arc<dyn Codec>,
        ready_notify: Arc<Notify>,
    ) -> Self {
        Self { dispatcher, codec, ready_notify, notified: AtomicBool::new(false) }
    }

    fn signal_ready(&self) {
        if !self.notified.swap(true, Ordering::SeqCst) {
            self.ready_notify.notify_one();
        }
    }
}

#[async_trait]
impl MessageListener for SocketHandler {
    async fn on_message(&self, frame: &Frame) -> FlareResult<Option<Frame>> {
        if let Some(ref cmd) = frame.command {
            match &cmd.r#type {
                Some(CommandType::System(sys_cmd)) => {
                    let t = sys_cmd.r#type;
                    if t == SystemType::ConnectAck as i32
                        || t == SystemType::Error as i32
                        || t == SystemType::Kicked as i32
                    {
                        self.signal_ready();
                    }
                }
                Some(CommandType::Message(_)) => {
                    self.signal_ready();
                }
                _ => {}
            }
        }

        let Some(ref cmd) = frame.command else { return Ok(None) };
        let Some(ref cmd_type) = cmd.r#type else { return Ok(None) };

        if let CommandType::Message(msg_cmd) = cmd_type {
            debug!(
                frame_id = %frame.message_id,
                msg_cmd_type = msg_cmd.r#type,
                payload_len = msg_cmd.payload.len(),
                "SocketHandler on_message: Message command"
            );
            if msg_cmd.r#type == 1 {
                return Ok(None);
            }
            if msg_cmd.r#type == 0 || msg_cmd.r#type == 2 {
                match self.codec.decode_server(&msg_cmd.payload) {
                    Ok(sp) => {
                        info!(
                            frame_id = %frame.message_id,
                            "received push message, dispatching to bus"
                        );
                        self.dispatcher.dispatch(sp).await
                    }
                    Err(e) => {
                        warn!(
                            frame_id = %frame.message_id,
                            payload_len = msg_cmd.payload.len(),
                            error = %e,
                            "payload decode as ServerPacket failed, ignored"
                        );
                    }
                }
            }
        }
        Ok(None)
    }

    async fn on_connect(&self) -> FlareResult<()> {
        self.dispatcher.bus().publish(SdkEvent::Connected);
        Ok(())
    }

    async fn on_disconnect(&self, reason: Option<&str>) -> FlareResult<()> {
        self.dispatcher.bus().publish(SdkEvent::Disconnected {
            reason: reason.unwrap_or("unknown").to_string(),
        });
        Ok(())
    }

    async fn on_error(&self, error: &str) -> FlareResult<()> {
        self.dispatcher.bus().publish(SdkEvent::ServerError {
            code: -1,
            message: error.to_string(),
        });
        Ok(())
    }
}
