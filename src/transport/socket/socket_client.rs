use std::sync::Arc;
use std::time::Duration;

use flare_core::client::builder::flare::{FlareClient, FlareClientBuilder, MessageListener};
use flare_core::common::config_types::TransportProtocol as CoreTransport;
use flare_core::common::device::{DeviceInfo, DevicePlatform};
use tokio::sync::{Mutex, Notify};
use tracing::info;

use crate::client::config::SdkConfig;
use crate::error::{SdkError, Result};
use crate::protocol::{Codec, ProtobufCodec, PacketSender};

/// 等待 CONNACK 的默认超时（秒）
const CONNACK_WAIT_SECS: u64 = 10;

/// Socket 传输层 — 基于 flare-core 的多协议长连接封装
///
/// 底层通过 flare-core `FlareClient` 实现，自动支持：
/// - **WebSocket** — 默认传输协议
/// - **QUIC** — 配置 `quic_url` 后自动启用
/// - **协议竞速** — 同时尝试 WebSocket 和 QUIC，使用最先成功建连的协议
///
/// # 协议竞速
///
/// 当同时配置了 `ws_url`（WebSocket）和 `quic_url`（QUIC）时，
/// 底层会并行发起两种协议的连接尝试，采用先成功者作为实际传输通道。
/// 对上层完全透明 — 无论使用哪种协议，API 和数据流完全一致。
///
/// # CONNACK 等待
///
/// `connect()` 在 `FlareClient.build_with_race()` 之后会等待服务端的 CONNACK
/// 响应到达，确保服务端已完成认证和连接上下文注册，再返回给上层。
/// 这样 `bootstrap()` 发出的同步请求不会因 "not authenticated" 被拒绝。
pub struct SocketTransport {
    config: SdkConfig,
    sender: Arc<PacketSender>,
    client: Arc<Mutex<Option<FlareClient>>>,
}

impl SocketTransport {
    pub fn new(config: SdkConfig) -> Self {
        Self::with_codec(config, Arc::new(ProtobufCodec))
    }

    pub fn with_codec(config: SdkConfig, codec: Arc<dyn Codec>) -> Self {
        let client: Arc<Mutex<Option<FlareClient>>> = Arc::new(Mutex::new(None));
        let sender = Arc::new(PacketSender::new(client.clone(), codec));
        Self { config, sender, client }
    }

    pub fn sender(&self) -> &Arc<PacketSender> {
        &self.sender
    }

    /// 建立连接并等待 CONNACK
    ///
    /// 流程：
    /// 1. 创建 `ready_notify` 并传给 `SocketHandler`
    /// 2. `build_with_race()` 建立 WebSocket/QUIC 连接 + 发送 CONNECT
    /// 3. 等待 `SocketHandler.on_message()` 收到服务端首帧（CONNACK），发出 ready 信号
    /// 4. 返回给上层，此时服务端已完成认证与上下文注册
    pub async fn connect(
        &self,
        user_id: &str,
        token: &str,
        listener: Arc<dyn MessageListener>,
        ready_notify: Arc<Notify>,
    ) -> Result<()> {
        let device = DeviceInfo::new(
            format!("sdk-{}-{}", user_id, std::process::id()),
            DevicePlatform::PC,
        )
        .with_model("FlareSDK".to_string())
        .with_app_version("1.0.0".to_string());

        let ws_url = self.config.ws_url.as_deref()
            .ok_or_else(|| SdkError::Config("ws_url not configured".into()))?;

        let mut builder = FlareClientBuilder::new(ws_url)
            .with_user_id(user_id.to_string())
            .with_token(token.to_string())
            .with_listener(listener)
            .with_device_info(device.clone())
            .with_connect_timeout(Duration::from_secs(self.config.connect_timeout_secs()));

        if let Some(secs) = self.config.reconnect_interval_secs {
            builder = builder.with_reconnect_interval(Duration::from_secs(secs));
        }
        if let Some(max) = self.config.max_reconnect_attempts {
            builder = builder.with_max_reconnect_attempts(Some(max));
        }

        if let Some(ref quic_url) = self.config.quic_url {
            builder = builder
                .with_protocol_url(CoreTransport::QUIC, quic_url.clone())
                .with_protocol_url(CoreTransport::WebSocket, ws_url.to_string())
                .with_protocol_race(vec![CoreTransport::QUIC, CoreTransport::WebSocket]);

            info!(ws = ws_url, quic = %quic_url, "protocol race enabled (WebSocket + QUIC)");
        }

        let flare_client = builder.build_with_race().await
            .map_err(|e| SdkError::ConnectionFailed(e.to_string()))?;

        *self.client.lock().await = Some(flare_client);

        // 等待 CONNACK（SocketHandler 收到首帧后会 notify_one）
        let wait = Duration::from_secs(CONNACK_WAIT_SECS);
        tokio::time::timeout(wait, ready_notify.notified())
            .await
            .map_err(|_| SdkError::ConnectionFailed(
                format!("CONNACK not received within {wait:?}"),
            ))?;

        info!(user_id, "socket connected (CONNACK received)");
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        let _ = self.client.lock().await.take();
        info!("socket disconnected");
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        self.client.lock().await.as_ref().map(|c| c.is_connected()).unwrap_or(false)
    }
}
