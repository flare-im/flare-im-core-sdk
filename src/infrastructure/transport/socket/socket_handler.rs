//! 与 flare-signaling/gateway 协议对齐：处理 PayloadCommand（Message/Event/Ack/Data）。
//! 网关下发：MESSAGE 推送=Payload(type=Message)、SendAck=Payload(type=Ack, payload=encoded Ack(Send))。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::shared::error::{Result, TENANT_UNAVAILABLE};
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

use crate::infrastructure::protocol::downlink::DownlinkPayload;
use crate::infrastructure::protocol::{Codec, MAX_DOWNLINK_PAYLOAD_BYTES};
use crate::kernel::event::{ConnectionEvent, SdkEvent};
use crate::runtime::Dispatcher;

/// Socket 推送处理器 — 实现 flare-core `MessageListener`
pub struct SocketHandler {
    dispatcher: Arc<Dispatcher>,
    codec: Arc<dyn Codec>,
    ready_notify: Arc<Notify>,
    notified: AtomicBool,
    /// 已向总线报过「租户不可用」的踢线：KICKED 帧与随后的断开原因可能都带着它，只报一次。
    tenant_unavailable_reported: AtomicBool,
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
            tenant_unavailable_reported: AtomicBool::new(false),
        }
    }

    fn signal_ready(&self) {
        if !self.notified.swap(true, Ordering::SeqCst) {
            self.ready_notify.notify_one();
        }
    }

    /// 服务端以 `TENANT_UNAVAILABLE` 踢线：交给应用一个 `KickedOff`，会话随之终止，
    /// 不自动重连、不刷新令牌（租户恢复前两者都只会被网关再拒一次）。
    fn report_tenant_unavailable(&self) {
        if self
            .tenant_unavailable_reported
            .swap(true, Ordering::SeqCst)
        {
            return;
        }
        warn!("server kicked the connection: tenant unavailable");
        self.dispatcher
            .bus()
            .publish(SdkEvent::Connection(ConnectionEvent::KickedOff {
                reason: TENANT_UNAVAILABLE.to_string(),
            }));
    }
}

/// KICKED 帧是否因租户不可用：原因在 `message`，网关也会放一份在 `metadata.reason`。
fn is_tenant_unavailable_kick(
    sys_cmd: &flare_core::common::protocol::flare::core::commands::SystemCommand,
) -> bool {
    sys_cmd.message.trim() == TENANT_UNAVAILABLE
        || sys_cmd
            .metadata
            .get("reason")
            .is_some_and(|reason| reason.as_slice() == TENANT_UNAVAILABLE.as_bytes())
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
                    } else if t == SystemType::Kicked as i32 && is_tenant_unavailable_kick(sys_cmd)
                    {
                        self.report_tenant_unavailable();
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
                if payload_exceeds_budget(&payload, frame.message_id.as_str(), pt, "ack") {
                    return Ok(None);
                }
                if let Ok(ack) = Ack::decode(payload.as_slice()) {
                    match ack.payload {
                        Some(AckPayload::Send(send_ack)) => {
                            let accepted =
                                send_ack.result.as_ref().and_then(|result| match result {
                                    flare_proto::common::send_ack::Result::Accepted(accepted) => {
                                        Some(accepted)
                                    }
                                    _ => None,
                                });
                            let error = send_ack.result.as_ref().and_then(|result| match result {
                                flare_proto::common::send_ack::Result::Error(error) => Some(error),
                                _ => None,
                            });
                            debug!(
                                frame_id = %frame.message_id,
                                client_msg_id = %send_ack.client_msg_id,
                                server_msg_id = %accepted.map(|a| a.server_msg_id.as_str()).unwrap_or_default(),
                                conversation_seq = accepted.map(|a| a.conversation_seq).unwrap_or_default(),
                                error_code = error.map(|e| e.code).unwrap_or_default(),
                                error_message = %error.map(|e| e.message.as_str()).unwrap_or_default(),
                                "received SendAck (PayloadCommand Ack), dispatching to bus"
                            );
                            let payload = DownlinkPayload::SendAck(send_ack.clone());
                            if let Err(error) = self.dispatcher.dispatch(payload).await {
                                warn!(
                                    frame_id = %frame.message_id,
                                    client_msg_id = %send_ack.client_msg_id,
                                    error = %error,
                                    "dispatch SendAck to bus failed"
                                );
                            }
                        }
                        Some(AckPayload::Event(event_ack)) => {
                            debug!(
                                frame_id = %frame.message_id,
                                event_id = %event_ack.event_id,
                                attributes = ?event_ack.attributes,
                                "received EventAck (PayloadCommand Ack)"
                            );
                        }
                        Some(_) | None => {}
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
                if payload_exceeds_budget(&payload, frame.message_id.as_str(), pt, "downlink") {
                    return Ok(None);
                }
                match self.codec.decode_server_payload(pt, &payload) {
                    Ok(downlink) => {
                        debug!(
                            frame_id = %frame.message_id,
                            payload_type = pt,
                            "received push/data/event frame, dispatching to bus"
                        );
                        if let Err(error) = self.dispatcher.dispatch(downlink).await {
                            warn!(
                                frame_id = %frame.message_id,
                                payload_type = pt,
                                error = %error,
                                "dispatch push/data/event frame to bus failed"
                            );
                        }
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
        self.signal_ready();
        self.dispatcher.bus().publish(SdkEvent::Connection(
            crate::kernel::event::ConnectionEvent::Connected,
        ));
        Ok(())
    }

    async fn on_disconnect(&self, reason: Option<&str>) -> Result<()> {
        let reason = reason.unwrap_or("unknown").to_string();
        let lower = reason.to_lowercase();
        // 被踢语义由 flare-core 在协商完成后通过 Disconnected(reason) 下发，此处仅根据 reason 映射
        if reason.contains(TENANT_UNAVAILABLE) {
            self.report_tenant_unavailable();
        } else if lower.contains("kick")
            || lower.contains("设备冲突")
            || lower.contains("device_conflict")
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

fn payload_exceeds_budget(payload: &[u8], frame_id: &str, payload_type: i32, label: &str) -> bool {
    if payload.len() <= MAX_DOWNLINK_PAYLOAD_BYTES {
        return false;
    }
    warn!(
        frame_id = %frame_id,
        payload_type,
        payload_len = payload.len(),
        max_payload_len = MAX_DOWNLINK_PAYLOAD_BYTES,
        label,
        "payload exceeds downlink budget, ignored"
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::notification::{
        NotificationHandlerRegistry, NotificationInboundPipeline,
    };
    use crate::application::services::{EventDeduper, MessageDeduper};
    use crate::infrastructure::protocol::ProtobufCodec;
    use crate::kernel::event::EventBus;
    use crate::spi::metrics::MetricsRecorder;
    use flare_core::common::protocol::{Reliability, builder, frame_with_system_command};
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    fn handler(bus: &EventBus) -> SocketHandler {
        let dispatcher = Dispatcher::new(
            bus.clone(),
            None,
            None,
            None,
            None,
            Arc::new(RwLock::new("u1".to_string())),
            EventDeduper::new(Some(64)),
            NotificationInboundPipeline::new(
                Arc::new(NotificationHandlerRegistry::new()),
                MessageDeduper::new(Some(64)),
                bus.clone(),
            ),
            MetricsRecorder::disabled(),
        );
        SocketHandler::new(
            Arc::new(dispatcher),
            Arc::new(ProtobufCodec),
            Arc::new(Notify::new()),
        )
    }

    async fn kicked_off_reasons(rx: &mut crate::kernel::event::EventReceiver) -> Vec<String> {
        let mut reasons = Vec::new();
        while let Ok(Ok(event)) =
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
        {
            if let SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) = event {
                reasons.push(reason);
            }
        }
        reasons
    }

    /// 服务端以 TENANT_UNAVAILABLE 踢线：应用收到一次 KickedOff（会话随之终止、不重连），
    /// 紧跟着的传输断开不会再报第二次。
    #[tokio::test]
    async fn tenant_unavailable_kick_is_reported_once_as_kicked_off() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let handler = handler(&bus);
        let frame = frame_with_system_command(
            builder::kicked(
                TENANT_UNAVAILABLE.to_string(),
                Some(HashMap::from([(
                    "reason".to_string(),
                    TENANT_UNAVAILABLE.as_bytes().to_vec(),
                )])),
            ),
            Reliability::AtLeastOnce,
        );

        // 读循环先把 KICKED 帧交上来（EOF 之后才解析到也一样）
        handler.on_message(&frame).await.unwrap();
        assert_eq!(kicked_off_reasons(&mut rx).await, vec![TENANT_UNAVAILABLE]);

        // 随后的传输断开若也带着这个原因，不再报第二次
        handler
            .on_disconnect(Some(TENANT_UNAVAILABLE))
            .await
            .unwrap();
        assert!(kicked_off_reasons(&mut rx).await.is_empty());
    }

    /// 其他原因的踢线（设备冲突等）不走这条路：它们的去留由断开原因决定，保持原样。
    #[tokio::test]
    async fn other_kicks_are_not_reported_as_tenant_unavailable() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let handler = handler(&bus);
        let frame = frame_with_system_command(
            builder::kicked("DEVICE_KICKED".to_string(), None),
            Reliability::AtLeastOnce,
        );

        handler.on_message(&frame).await.unwrap();

        assert!(kicked_off_reasons(&mut rx).await.is_empty());
    }
}
