use std::sync::Arc;
use std::time::Duration;

use flare_core::client::builder::flare::{FlareClient, FlareClientBuilder, MessageListener};
use flare_core::common::config_types::TransportProtocol as CoreTransport;
use flare_core::common::device::{DeviceInfo, DevicePlatform};
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

use crate::client::config::SdkConfig;
use crate::error::{ErrorCode, FlareError, Result};
use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};

/// Socket 传输层 — 基于 flare-core 的多协议长连接封装
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
        Self {
            config,
            sender,
            client,
        }
    }

    pub fn sender(&self) -> &Arc<PacketSender> {
        &self.sender
    }

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

        let ws_url = self.config.ws_url.as_deref().ok_or_else(|| {
            FlareError::localized(ErrorCode::ConfigurationError, "ws_url not configured")
        })?;

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

        let flare_client = builder
            .build_with_race()
            .await
            .map_err(|e| FlareError::connection_failed(e.to_string()))?;

        if let Some(old_client) = self.client.lock().await.take()
            && let Err(error) = old_client.disconnect().await
        {
            warn!(error = %error, "closing stale socket client before reconnect failed");
        }

        *self.client.lock().await = Some(flare_client);

        let wait = Duration::from_secs(self.config.connect_timeout_secs().max(1));
        if tokio::time::timeout(wait, ready_notify.notified())
            .await
            .is_err()
        {
            let timed_out_client = self.client.lock().await.take();
            if let Some(client) = timed_out_client
                && let Err(error) = client.disconnect().await
            {
                warn!(error = %error, "closing socket after CONNACK timeout failed");
            }
            return Err(FlareError::connection_failed(format!(
                "CONNACK not received within {wait:?}"
            )));
        }

        info!(user_id, "socket connected (CONNACK received)");
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        if let Some(client) = self.client.lock().await.take()
            && let Err(error) = client.disconnect().await
        {
            warn!(error = %error, "socket client disconnect failed");
        }
        info!("socket disconnected");
        Ok(())
    }

    pub async fn is_connected(&self) -> bool {
        self.client
            .lock()
            .await
            .as_ref()
            .map(|c| c.is_connected())
            .unwrap_or(false)
    }
}
