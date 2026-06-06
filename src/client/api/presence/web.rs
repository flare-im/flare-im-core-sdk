//! Web/WASM presence facade — gateway REST + event-backed cache.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::core::event::{EventBus, MessageEvent, SdkEvent};
use crate::infrastructure::transport::http::http_client::HttpRequestContext;
use crate::infrastructure::transport::http::{
    HttpApiResponse, HttpClient, unwrap_api_response, unwrap_void_api_response,
};
use crate::shared::error::{ErrorCode, FlareError, Result};

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchGetUserPresenceHttpResponse {
    presences: HashMap<String, UserPresenceDto>,
}

#[derive(Debug, Serialize)]
struct BatchGetUserPresenceHttpRequest {
    user_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LogoutPresenceHttpRequest {
    conversation_id: String,
}

#[derive(Clone)]
pub struct PresenceApi {
    http: HttpClient,
    current_user_id: Arc<RwLock<String>>,
    cache: Arc<RwLock<HashMap<String, UserPresenceDto>>>,
    subscribed_user_ids: Arc<Mutex<HashSet<String>>>,
}

impl PresenceApi {
    pub fn new(
        http_base_url: impl Into<String>,
        current_user_id: Arc<RwLock<String>>,
        default_tenant_id: impl Into<String>,
        http_request_context: Arc<HttpRequestContext>,
        bus: EventBus,
    ) -> Self {
        let cache = Arc::new(RwLock::new(HashMap::new()));
        let cache_for_task = cache.clone();
        let mut rx = bus.subscribe();
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let ev = match rx.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if let SdkEvent::Message(MessageEvent::PresenceChanged {
                    conversation_id,
                    event,
                }) = ev
                {
                    let user_id = event.user_id.clone();
                    let status = event.status.clone();
                    let is_online = !status.eq_ignore_ascii_case("offline")
                        && !status.eq_ignore_ascii_case("invisible");
                    let dto = UserPresenceDto {
                        user_id: user_id.clone(),
                        is_online,
                        status,
                        last_seen_ms: chrono::Utc::now().timestamp_millis(),
                        devices: vec![DevicePresenceDto {
                            device_id: String::new(),
                            platform: String::new(),
                            last_active_time_ms: chrono::Utc::now().timestamp_millis(),
                            conversation_id,
                            gateway_id: String::new(),
                            server_id: String::new(),
                        }],
                    };
                    cache_for_task.write().await.insert(user_id, dto);
                }
            }
        });

        let _ = crate::shared::util::normalize_tenant_id(default_tenant_id.into());
        Self {
            http: HttpClient::with_context(http_base_url, http_request_context),
            current_user_id,
            cache,
            subscribed_user_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn offline(user_id: &str) -> UserPresenceDto {
        UserPresenceDto {
            user_id: user_id.to_string(),
            is_online: false,
            status: "offline".to_string(),
            last_seen_ms: 0,
            devices: Vec::new(),
        }
    }

    async fn fetch_user_presence(&self, user_id: &str) -> Result<UserPresenceDto> {
        let path = format!("/api/v1/presence/users/{user_id}");
        let body: HttpApiResponse<UserPresenceDto> = self.http.get(&path, None).await?;
        let dto = unwrap_api_response(body, "get user presence")?;
        self.cache
            .write()
            .await
            .insert(user_id.to_string(), dto.clone());
        Ok(dto)
    }

    pub async fn get_user_presence(&self, user_id: &str) -> Result<UserPresenceDto> {
        let uid = user_id.trim();
        if uid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.presence.user_id_required",
            ));
        }
        if let Some(cached) = self.cache.read().await.get(uid).cloned() {
            return Ok(cached);
        }
        match self.fetch_user_presence(uid).await {
            Ok(dto) => Ok(dto),
            Err(_) => Ok(Self::offline(uid)),
        }
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

        let mut out = HashMap::new();
        let mut missing = Vec::new();
        {
            let cache = self.cache.read().await;
            for id in &ids {
                if let Some(dto) = cache.get(id) {
                    out.insert(id.clone(), dto.clone());
                } else {
                    missing.push(id.clone());
                }
            }
        }

        if !missing.is_empty() {
            let req = BatchGetUserPresenceHttpRequest {
                user_ids: missing.clone(),
            };
            match self
                .http
                .post::<_, HttpApiResponse<BatchGetUserPresenceHttpResponse>>(
                    "/api/v1/presence/users/batch",
                    &req,
                )
                .await
            {
                Ok(body) => {
                    if let Ok(data) = unwrap_api_response(body, "batch get user presence") {
                        let mut cache = self.cache.write().await;
                        for (user_id, dto) in data.presences {
                            cache.insert(user_id.clone(), dto.clone());
                            out.insert(user_id, dto);
                        }
                    }
                }
                Err(_) => {
                    for id in missing {
                        out.entry(id.clone()).or_insert_with(|| Self::offline(&id));
                    }
                }
            }
        }

        for id in ids {
            out.entry(id.clone()).or_insert_with(|| Self::offline(&id));
        }
        Ok(out)
    }

    pub async fn logout_current_device_presence(&self) -> Result<()> {
        let user_id = self.current_user_id.read().await.trim().to_string();
        if user_id.is_empty() {
            return Ok(());
        }

        let presence = self.get_user_presence(&user_id).await?;
        let Some(device) = presence
            .devices
            .into_iter()
            .filter(|device| !device.conversation_id.trim().is_empty())
            .max_by_key(|device| device.last_active_time_ms)
        else {
            self.cache.write().await.clear();
            return Ok(());
        };

        let req = LogoutPresenceHttpRequest {
            conversation_id: device.conversation_id,
        };
        let body: HttpApiResponse<()> = self.http.post("/api/v1/presence/logout", &req).await?;
        unwrap_void_api_response(body, "logout presence")?;
        self.cache.write().await.clear();
        Ok(())
    }

    pub async fn subscribe_user_presence(&self, user_ids: Vec<String>) -> Result<()> {
        let mut subscribed = self.subscribed_user_ids.lock().await;
        for user_id in user_ids {
            let uid = user_id.trim();
            if !uid.is_empty() {
                subscribed.insert(uid.to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_presence_defaults_to_offline_status() {
        let dto = PresenceApi::offline("alice");
        assert_eq!(dto.user_id, "alice");
        assert!(!dto.is_online);
        assert_eq!(dto.status, "offline");
        assert!(dto.devices.is_empty());
    }

    #[test]
    fn batch_request_serializes_camel_case_user_ids() {
        let payload = BatchGetUserPresenceHttpRequest {
            user_ids: vec!["alice".into(), "bob".into()],
        };
        let json = serde_json::to_value(payload).expect("serialize batch request");
        assert_eq!(
            json.get("userIds")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(2)
        );
    }

    #[test]
    fn user_presence_dto_deserializes_gateway_http_payload() {
        let json = r#"{
            "userId": "alice",
            "isOnline": true,
            "status": "online",
            "lastSeenMs": 42,
            "devices": [{
                "deviceId": "d1",
                "platform": "web",
                "lastActiveTimeMs": 99,
                "conversationId": "c1",
                "gatewayId": "gw1",
                "serverId": "srv1"
            }]
        }"#;
        let dto: UserPresenceDto = serde_json::from_str(json).expect("deserialize presence dto");
        assert_eq!(dto.user_id, "alice");
        assert!(dto.is_online);
        assert_eq!(dto.devices[0].conversation_id, "c1");
    }
}
