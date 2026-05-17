//! 与 flare-signaling/gateway 协议对齐：处理 PayloadCommand（Message/Event/Ack/Data）。
//! 网关下发：MESSAGE 推送=Payload(type=Message)、SendAck=Payload(type=Ack, payload=encoded Ack(Send))。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::Result;
use async_trait::async_trait;
use flare_core::client::builder::flare::MessageListener;
use flare_core::common::compression::CompressionUtil;
use flare_core::common::protocol::Frame;
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::flare::core::commands::payload_command::Type as PayloadType;
use flare_core::common::protocol::flare::core::commands::system_command::Type as SystemType;
use flare_proto::common::Ack;
use flare_proto::common::ack::Payload as AckPayload;
use prost::Message;
use tokio::sync::Notify;
use tracing::{debug, warn};

use crate::core::Dispatcher;
use crate::event::{ConnectionEvent, SdkEvent};
use crate::infrastructure::protocol::Codec;
use crate::infrastructure::protocol::downlink::DownlinkPayload;

/// Socket 推送处理器 — 实现 flare-core `MessageListener`
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
        Self {
            dispatcher,
            codec,
            ready_notify,
            notified: AtomicBool::new(false),
        }
    }

    fn signal_ready(&self) {
        if !self.notified.swap(true, Ordering::SeqCst) {
            self.ready_notify.notify_one();
        }
    }
}

#[async_trait]
impl MessageListener for SocketHandler {
    async fn on_message(&self, frame: &Frame) -> Result<Option<Frame>> {
        if let Some(ref cmd) = frame.command {
            match &cmd.r#type {
                Some(CommandType::System(sys_cmd)) => {
                    let t = sys_cmd.r#type;
                    if t == SystemType::ConnectAck as i32 {
                        self.signal_ready();
                    }
                }
                Some(CommandType::Payload(_)) => {
                    self.signal_ready();
                }
                _ => {}
            }
        }

        let Some(ref cmd) = frame.command else {
            return Ok(None);
        };
        let Some(ref cmd_type) = cmd.r#type else {
            return Ok(None);
        };
        tracing::debug!(
            frame_id = %frame.message_id,
            cmd = ?cmd_type,
            "on_message: receive command",
        );

        // 与 flare-signaling gateway 一致：Frame.command 为 Payload(PayloadCommand)，type 1=Message 2=Event 3=Ack 4=Data
        if let CommandType::Payload(payload_cmd) = cmd_type {
            let pt = payload_cmd.r#type;
            // PayloadType::Ack (3)：网关对 MESSAGE 的回执，payload 为 flare_proto::common::Ack(SendAck)
            if pt == PayloadType::Ack as i32 {
                let payload = ensure_decompressed_payload(&payload_cmd.payload);
                if let Ok(ack) = Ack::decode(payload.as_slice()) {
                    match ack.payload {
                        Some(AckPayload::Send(send_ack)) => {
                            debug!(
                                frame_id = %frame.message_id,
                                client_msg_id = %send_ack.client_msg_id,
                                server_msg_id = %send_ack.server_msg_id,
                                success = send_ack.success,
                                error_code = send_ack.error_code,
                                error_message = %send_ack.error_message,
                                "received SendAck (PayloadCommand Ack), dispatching to bus"
                            );
                            let payload = DownlinkPayload::SendAck(send_ack.clone());
                            let _ = self.dispatcher.dispatch(payload).await;
                        }
                        Some(AckPayload::Event(event_ack)) => {
                            debug!(
                                frame_id = %frame.message_id,
                                event_id = %event_ack.event_id,
                                metadata = ?event_ack.metadata,
                                "received EventAck (PayloadCommand Ack)"
                            );
                        }
                        _ => {}
                    }
                } else {
                    warn!(
                        frame_id = %frame.message_id,
                        payload_len = payload_cmd.payload.len(),
                        "payload decode as Ack failed, ignored"
                    );
                }
                return Ok(None);
            }
            // PayloadType::Message (1) / Data (4) / Event (2)：内层为 MessagePush / Event / EventEnvelope / DataPacket…（与网关下行约定一致）
            if pt == PayloadType::Message as i32
                || pt == PayloadType::Data as i32
                || pt == PayloadType::Event as i32
            {
                let payload = ensure_decompressed_payload(&payload_cmd.payload);
                match self.codec.decode_server(&payload) {
                    Ok(downlink) => {
                        debug!(
                            frame_id = %frame.message_id,
                            payload_type = pt,
                            "received push/data/event frame, dispatching to bus"
                        );
                        let _ = self.dispatcher.dispatch(downlink).await;
                    }
                    Err(e) => {
                        warn!(
                            frame_id = %frame.message_id,
                            payload_len = payload_cmd.payload.len(),
                            error = %e,
                            "payload decode as downlink failed, ignored"
                        );
                    }
                }
            }
        }
        Ok(None)
    }

    async fn on_connect(&self) -> Result<()> {
        self.dispatcher.bus().publish(SdkEvent::Connection(
            crate::event::ConnectionEvent::Connected,
        ));
        Ok(())
    }

    async fn on_disconnect(&self, reason: Option<&str>) -> Result<()> {
        let reason = reason.unwrap_or("unknown").to_string();
        let lower = reason.to_lowercase();
        // 被踢语义由 flare-core 在协商完成后通过 Disconnected(reason) 下发，此处仅根据 reason 映射
        if lower.contains("kick") || lower.contains("设备冲突") || lower.contains("device_conflict")
        {
            self.dispatcher
                .bus()
                .publish(SdkEvent::Connection(ConnectionEvent::KickedOff {
                    reason: reason.clone(),
                }));
        }
        self.dispatcher
            .bus()
            .publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
                reason,
            }));
        Ok(())
    }

    async fn on_error(&self, error: &str) -> Result<()> {
        let msg = error.to_string();
        let lower = msg.to_lowercase();
        if lower.contains("token_expired")
            || lower.contains("token expired")
            || lower.contains("401")
            || lower.contains("credential expired")
        {
            self.dispatcher
                .bus()
                .publish(SdkEvent::Connection(ConnectionEvent::TokenExpired {
                    message: msg,
                }));
        } else {
            self.dispatcher
                .bus()
                .publish(SdkEvent::Connection(ConnectionEvent::ServerError {
                    code: -1,
                    message: msg,
                }));
        }
        Ok(())
    }
}

/// 若 payload 为 Gzip 等压缩数据则解压，否则返回原数据（与 chatroom_client 一致，协商 Gzip 时服务端可能下发的压缩 payload）。
fn ensure_decompressed_payload(payload: &[u8]) -> Vec<u8> {
    match CompressionUtil::auto_decompress(payload) {
        Ok((decompressed, _)) => decompressed,
        Err(_) => payload.to_vec(),
    }
}
