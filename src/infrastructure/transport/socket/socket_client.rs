use std::sync::Arc;
use std::time::Duration;

use flare_core::client::builder::flare::{FlareClient, FlareClientBuilder, MessageListener};
#[cfg(not(target_arch = "wasm32"))]
use flare_core::common::config_types::TransportProtocol as CoreTransport;
use flare_core::common::device::{DeviceInfo, DevicePlatform};
use flare_core::common::protocol::SerializationFormat;
use flare_core::common::{HeartbeatAppState, HeartbeatConfig};
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

use crate::client::config::SdkConfig;
use crate::infrastructure::protocol::{Codec, PacketSender, ProtobufCodec};
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::timeout;

fn sdk_device_info(user_id: &str) -> DeviceInfo {
    #[cfg(not(target_arch = "wasm32"))]
    let device_id = format!("sdk-{}-{}", user_id, std::process::id());
    #[cfg(target_arch = "wasm32")]
    let device_id = format!("sdk-{}-web-{}", user_id, uuid::Uuid::new_v4());

    #[cfg(not(target_arch = "wasm32"))]
    let platform = DevicePlatform::PC;
    #[cfg(not(target_arch = "wasm32"))]
    let model = "FlareSDK".to_string();
    #[cfg(target_arch = "wasm32")]
    let platform = DevicePlatform::Web;
    #[cfg(target_arch = "wasm32")]
    let model = "FlareSDK-Web".to_string();

    DeviceInfo::new(device_id, platform)
        .with_model(model)
        .with_app_version("1.0.0".to_string())
}

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
        let device = sdk_device_info(user_id);

        let ws_url = self.config.ws_url.as_deref().ok_or_else(|| {
            FlareError::localized(ErrorCode::ConfigurationError, "ws_url not configured")
        })?;
        let primary_url = self
            .config
            .primary_connect_url()
            .unwrap_or_else(|| ws_url.to_string());

        let mut builder = FlareClientBuilder::new(&primary_url)
            .with_user_id(user_id.to_string())
            .with_token(token.to_string())
            .with_listener(listener)
            .with_device_info(device)
            .with_format(SerializationFormat::Protobuf)
            .with_connect_timeout(Duration::from_secs(self.config.connect_timeout_secs()));

        if let Some(secs) = self.config.reconnect_interval_secs {
            builder = builder.with_reconnect_interval(Duration::from_secs(secs));
        }
        if let Some(max) = self.config.max_reconnect_attempts {
            builder = builder.with_max_reconnect_attempts(Some(max));
        }

        let policy = self.config.effective_transport_policy();
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(race_order), Some(quic_url)) = (
            self.config.effective_protocol_race_order(),
            self.config.quic_url.as_ref(),
        ) {
            for protocol in &race_order {
                let url = match protocol {
                    CoreTransport::WebSocket => ws_url.to_string(),
                    CoreTransport::QUIC => quic_url.clone(),
                    _ => continue,
                };
                builder = builder.with_protocol_url(*protocol, url);
            }
            builder = builder.with_protocol_race(race_order.clone());
            info!(
                ws = ws_url,
                quic = %quic_url,
                race_order = ?race_order,
                policy = ?policy,
                "protocol race enabled"
            );
        } else {
            info!(
                ws = ws_url,
                primary = %primary_url,
                policy = ?policy,
                default_transport = ?self.config.default_transport,
                "single transport (WebSocket entry)"
            );
        }
        #[cfg(target_arch = "wasm32")]
        {
            info!(
                ws = ws_url,
                primary = %primary_url,
                policy = ?policy,
                "wasm websocket transport (flare-core FlareClientBuilder)"
            );
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
        if timeout(wait, ready_notify.notified()).await.is_err() {
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

    pub async fn update_heartbeat_config(&self, config: HeartbeatConfig) -> Result<()> {
        if let Some(client) = self.client.lock().await.as_ref() {
            client.update_heartbeat_config(config).await;
        }
        Ok(())
    }

    pub async fn set_heartbeat_app_state(&self, state: HeartbeatAppState) -> Result<()> {
        if let Some(client) = self.client.lock().await.as_ref() {
            client.set_heartbeat_app_state(state).await;
        }
        Ok(())
    }

    pub async fn set_heartbeat_nat_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        if let Some(client) = self.client.lock().await.as_ref() {
            client.set_heartbeat_nat_timeout(timeout).await;
        }
        Ok(())
    }

    pub async fn heartbeat_effective_interval(&self) -> Option<Duration> {
        match self.client.lock().await.as_ref() {
            Some(client) => Some(client.heartbeat_effective_interval().await),
            None => None,
        }
    }

    pub async fn is_connected(&self) -> bool {
        match self.client.lock().await.as_ref() {
            None => false,
            #[cfg(not(target_arch = "wasm32"))]
            Some(client) => client.is_connected(),
            #[cfg(target_arch = "wasm32")]
            Some(client) => client.is_connected_async().await,
        }
    }
}
