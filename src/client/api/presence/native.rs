//! 用户在线状态 gRPC API（`flare.signaling.online.OnlineService`）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};

use crate::client::api::session_guard::SessionGuard;
use crate::core::event::{EventBus, MessageEvent, SdkEvent};
use crate::infrastructure::transport::http::http_client::HttpRequestContext;
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_grpc_proto::signaling::online::online_service_client::OnlineServiceClient;
use flare_grpc_proto::signaling::online::{
    BatchGetUserPresenceRequest, GetUserPresenceRequest, LogoutRequest,
    SubscribeUserPresenceRequest, UserPresence,
};
use flare_proto::common::PresenceHintPacket;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePresenceDto {
    pub device_id: String,
    pub platform: String,
    pub last_active_time_ms: i64,
    pub conversation_id: String,
    pub gateway_id: String,
    pub server_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPresenceDto {
    pub user_id: String,
    pub is_online: bool,
    pub status: String,
    pub last_seen_ms: i64,
    pub devices: Vec<DevicePresenceDto>,
}

#[derive(Clone)]
pub struct PresenceApi {
    session_guard: SessionGuard,
    endpoint: String,
    channel: Arc<Mutex<Option<Channel>>>,
    current_user_id: Arc<RwLock<String>>,
    default_tenant_id: String,
    http_request_context: Arc<HttpRequestContext>,
    bus: EventBus,
    subscribed_user_ids: Arc<Mutex<HashSet<String>>>,
}

impl PresenceApi {
    pub fn new(
        grpc_endpoint: impl Into<String>,
        current_user_id: Arc<RwLock<String>>,
        default_tenant_id: impl Into<String>,
        http_request_context: Arc<HttpRequestContext>,
        bus: EventBus,
    ) -> Self {
        Self {
            session_guard: SessionGuard::new(current_user_id.clone(), "presence"),
            endpoint: grpc_endpoint.into(),
            channel: Arc::new(Mutex::new(None)),
            current_user_id,
            default_tenant_id: crate::shared::util::normalize_tenant_id(default_tenant_id.into()),
            http_request_context,
            bus,
            subscribed_user_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    async fn connect(&self) -> Result<Channel> {
        let mut g = self.channel.lock().await;
        if g.is_none() {
            let ch = Endpoint::from_shared(self.endpoint.clone())
                .map_err(|e| FlareError::system(format!("online endpoint: {e}")))?
                .connect()
                .await
                .map_err(|e| FlareError::system(format!("online connect: {e}")))?;
            *g = Some(ch);
        }
        g.as_ref()
            .cloned()
            .ok_or_else(|| FlareError::system("online channel unavailable"))
    }

    async fn enrich_metadata<T>(&self, req: &mut tonic::Request<T>) -> Result<()> {
        let user_id = self.current_user_id.read().await.clone();
        if !user_id.trim().is_empty() {
            let v = MetadataValue::try_from(user_id.trim())
                .map_err(|e| FlareError::system(format!("x-user-id metadata: {e}")))?;
            req.metadata_mut().insert("x-user-id", v);
        }
        let tenant = crate::shared::util::normalize_tenant_id(self.default_tenant_id.trim());
        if !tenant.is_empty() {
            let v = MetadataValue::try_from(tenant.as_str())
                .map_err(|e| FlareError::system(format!("x-tenant-id metadata: {e}")))?;
            req.metadata_mut().insert("x-tenant-id", v);
        }
        let trace = uuid::Uuid::new_v4().to_string();
        if let Ok(v) = MetadataValue::try_from(trace.as_str()) {
            req.metadata_mut().insert("x-trace-id", v);
        }
        for (k, v) in self.http_request_context.build_headers().await {
            if k.eq_ignore_ascii_case("authorization")
                && let Ok(mv) = MetadataValue::try_from(v.as_str())
            {
                req.metadata_mut().insert("authorization", mv);
            }
        }
        Ok(())
    }

    async fn get_user_presence_unbound(&self, user_id: &str) -> Result<UserPresenceDto> {
        let uid = user_id.trim();
        if uid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.presence.user_id_required",
            ));
        }
        let ch = self.connect().await?;
        let mut client = OnlineServiceClient::new(ch);
        let mut req = tonic::Request::new(GetUserPresenceRequest {
            user_id: uid.to_string(),
        });
        self.enrich_metadata(&mut req).await?;
        let resp = client
            .get_user_presence(req)
            .await
            .map_err(|s| FlareError::system(format!("GetUserPresence: {}", s.message())))?
            .into_inner();
        resp.presence
            .map(user_presence_to_dto)
            .ok_or_else(|| FlareError::system("GetUserPresence: missing presence"))
    }

    pub async fn get_user_presence(&self, user_id: &str) -> Result<UserPresenceDto> {
        let uid = user_id.trim();
        if uid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.presence.user_id_required",
            ));
        }
        let api = self.clone();
        let uid = uid.to_string();
        self.session_guard
            .run(async move { api.get_user_presence_unbound(&uid).await })
            .await
    }

    pub async fn batch_get_user_presence(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, UserPresenceDto>> {
        let ids: Vec<String> = user_ids
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let api = self.clone();
        self.session_guard
            .run(async move {
                let ch = api.connect().await?;
                let mut client = OnlineServiceClient::new(ch);
                let mut req = tonic::Request::new(BatchGetUserPresenceRequest { user_ids: ids });
                api.enrich_metadata(&mut req).await?;
                let resp = client
                    .batch_get_user_presence(req)
                    .await
                    .map_err(|s| {
                        FlareError::system(format!("BatchGetUserPresence: {}", s.message()))
                    })?
                    .into_inner();
                Ok(resp
                    .presences
                    .into_iter()
                    .map(|(user_id, p)| (user_id, user_presence_to_dto(p)))
                    .collect())
            })
            .await
    }

    /// 主动注销当前 SDK 用户最近活跃的 Online 会话。
    ///
    /// 这是用户点击“退出登录”时的主动下线路径，不能只依赖 WebSocket 断开后的网关回调。
    pub async fn logout_current_device_presence(&self) -> Result<()> {
        let user_id = self.current_user_id.read().await.trim().to_string();
        if user_id.is_empty() {
            return Ok(());
        }
        self.logout_user_device_presence(&user_id).await
    }

    pub(crate) async fn logout_user_device_presence(&self, user_id: &str) -> Result<()> {
        let user_id = user_id.trim().to_string();
        if user_id.is_empty() {
            return Ok(());
        }

        let presence = self.get_user_presence_unbound(&user_id).await?;
        let Some(device) = presence
            .devices
            .into_iter()
            .filter(|device| !device.conversation_id.trim().is_empty())
            .max_by_key(|device| device.last_active_time_ms)
        else {
            return Ok(());
        };

        let ch = self.connect().await?;
        let mut client = OnlineServiceClient::new(ch);
        let mut req = tonic::Request::new(LogoutRequest {
            user_id,
            conversation_id: device.conversation_id,
        });
        self.enrich_metadata(&mut req).await?;
        client
            .logout(req)
            .await
            .map_err(|s| FlareError::system(format!("LogoutPresence: {}", s.message())))?;
        Ok(())
    }

    /// 订阅用户在线状态变化，并将事件发布到 SDK EventBus。
    pub async fn subscribe_user_presence(&self, user_ids: Vec<String>) -> Result<()> {
        let requested_ids: Vec<String> = user_ids
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if requested_ids.is_empty() {
            return Ok(());
        }
        let ids = {
            let subscribed = self.subscribed_user_ids.lock().await;
            let mut fresh = Vec::new();
            for id in requested_ids {
                if !subscribed.contains(&id) {
                    fresh.push(id);
                }
            }
            fresh
        };
        if ids.is_empty() {
            return Ok(());
        }
        let api = self.clone();
        let ids_for_request = ids.clone();
        let (mut stream, session_user_id) = self
            .session_guard
            .run_with_user(move |session_user_id| async move {
                let ch = api.connect().await?;
                let mut client = OnlineServiceClient::new(ch);
                let mut req = tonic::Request::new(SubscribeUserPresenceRequest {
                    user_ids: ids_for_request,
                });
                api.enrich_metadata(&mut req).await?;
                let stream = client
                    .subscribe_user_presence(req)
                    .await
                    .map_err(|s| {
                        FlareError::system(format!("SubscribeUserPresence: {}", s.message()))
                    })?
                    .into_inner();
                Ok((stream, session_user_id.unwrap_or_default()))
            })
            .await?;
        {
            let mut subscribed = self.subscribed_user_ids.lock().await;
            for id in &ids {
                subscribed.insert(id.clone());
            }
        }
        let bus = self.bus.clone();
        let subscribed_user_ids = self.subscribed_user_ids.clone();
        let session_guard = self.session_guard.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = session_guard.wait_until_session_changes(&session_user_id) => break,
                    message = stream.message() => {
                        match message {
                            Ok(Some(event)) => {
                                let occurred_at = timestamp_ms(event.timestamp);
                                let mut extra = HashMap::new();
                                extra.insert("deviceId".to_string(), event.device_id.clone());
                                extra.insert("lastSeenMs".to_string(), occurred_at.to_string());
                                bus.publish(SdkEvent::Message(MessageEvent::PresenceChanged {
                                    conversation_id: String::new(),
                                    event: PresenceHintPacket {
                                        user_id: event.user_id,
                                        status: if event.is_online {
                                            "online".to_string()
                                        } else {
                                            "offline".to_string()
                                        },
                                        device_id: Some(event.device_id),
                                        attributes: extra,
                                        occurred_at: Some(occurred_at),
                                    },
                                }));
                            }
                            Ok(None) => break,
                            Err(err) => {
                                tracing::warn!(%err, "presence subscription stream ended");
                                break;
                            }
                        }
                    }
                }
            }
            let mut subscribed = subscribed_user_ids.lock().await;
            for id in ids {
                subscribed.remove(&id);
            }
        });
        Ok(())
    }
}

fn timestamp_ms(ts: Option<prost_types::Timestamp>) -> i64 {
    ts.map(|t| (t.seconds * 1000) + i64::from(t.nanos / 1_000_000))
        .unwrap_or(0)
}

fn user_presence_to_dto(p: UserPresence) -> UserPresenceDto {
    let is_online = p.is_online;
    UserPresenceDto {
        user_id: p.user_id,
        is_online,
        status: if is_online { "online" } else { "offline" }.to_string(),
        last_seen_ms: timestamp_ms(p.last_seen),
        devices: p
            .devices
            .into_iter()
            .map(|d| DevicePresenceDto {
                device_id: d.device_id,
                platform: d.platform,
                last_active_time_ms: timestamp_ms(d.last_active_time),
                conversation_id: d.conversation_id,
                gateway_id: d.gateway_id,
                server_id: d.server_id,
            })
            .collect(),
    }
}
