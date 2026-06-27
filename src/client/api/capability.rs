//! 能力扩展 gRPC API（`flare.capability.v1.CapabilityService`）。
//!
//! 直连 `flare-capability` 或与接入网关转发的同一 gRPC 服务；生产环境通常经网关暴露，URI 指向网关后端。

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use flare_grpc_proto::capability::capability_service_client::CapabilityServiceClient;
#[cfg(not(target_arch = "wasm32"))]
use flare_grpc_proto::capability::{
    DispatchCapabilityRequest, GrantUserCapabilityRequest, ListUserCapabilitiesRequest,
    RevokeUserCapabilityRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::Mutex;
use tokio::sync::RwLock;
#[cfg(not(target_arch = "wasm32"))]
use tonic::metadata::MetadataValue;
#[cfg(not(target_arch = "wasm32"))]
use tonic::transport::{Channel, Endpoint};

#[cfg(not(target_arch = "wasm32"))]
use super::session_guard::SessionGuard;
#[cfg(not(target_arch = "wasm32"))]
use crate::extension::capability::rtc_ids;
use crate::infrastructure::transport::http::http_client::HttpRequestContext;
use crate::shared::error::{ErrorCode, FlareError, Result};

#[derive(Clone)]
pub struct CapabilityApi {
    #[cfg(not(target_arch = "wasm32"))]
    session_guard: SessionGuard,
    #[cfg(not(target_arch = "wasm32"))]
    endpoint: String,
    #[cfg(not(target_arch = "wasm32"))]
    channel: Arc<Mutex<Option<Channel>>>,
    #[cfg(not(target_arch = "wasm32"))]
    current_user_id: Arc<RwLock<String>>,
    #[cfg(not(target_arch = "wasm32"))]
    default_tenant_id: String,
    #[cfg(not(target_arch = "wasm32"))]
    http_request_context: Arc<HttpRequestContext>,
}

#[derive(Clone, Debug)]
pub struct RtcSfuSubscriptionRequest {
    pub conversation_id: String,
    pub room_id: String,
    pub subscriber_peer_id: String,
    pub track_id: String,
    pub enable: bool,
    pub media: Option<String>,
    pub preferred_layer: Option<String>,
    pub priority: u32,
    pub tenant_id: Option<String>,
}

impl CapabilityApi {
    pub fn new(
        grpc_endpoint: impl Into<String>,
        current_user_id: Arc<RwLock<String>>,
        default_tenant_id: impl Into<String>,
        http_request_context: Arc<HttpRequestContext>,
    ) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (
                grpc_endpoint.into(),
                current_user_id,
                default_tenant_id.into(),
                http_request_context,
            );
            Self {}
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self {
            session_guard: SessionGuard::new(current_user_id.clone(), "capability"),
            endpoint: grpc_endpoint.into(),
            channel: Arc::new(Mutex::new(None)),
            current_user_id,
            default_tenant_id: crate::shared::util::normalize_tenant_id(default_tenant_id.into()),
            http_request_context,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn connect(&self) -> Result<Channel> {
        let mut g = self.channel.lock().await;
        if g.is_none() {
            let ch = Endpoint::from_shared(self.endpoint.clone())
                .map_err(|e| FlareError::system(format!("capability endpoint: {e}")))?
                .connect()
                .await
                .map_err(|e| FlareError::system(format!("capability connect: {e}")))?;
            *g = Some(ch);
        }
        g.as_ref()
            .cloned()
            .ok_or_else(|| FlareError::system("capability channel unavailable"))
    }

    #[cfg(not(target_arch = "wasm32"))]
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

    #[cfg(not(target_arch = "wasm32"))]
    fn effective_tenant_id(&self, tenant_id: Option<&str>) -> String {
        tenant_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(crate::shared::util::normalize_tenant_id)
            .unwrap_or_else(|| crate::shared::util::normalize_tenant_id(&self.default_tenant_id))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn list_capabilities(&self) -> Result<Vec<CapabilityDescriptorDto>> {
        let api = self.clone();
        self.session_guard
            .run(async move {
                let ch = api.connect().await?;
                let mut client = CapabilityServiceClient::new(ch);
                let mut req =
                    tonic::Request::new(flare_grpc_proto::capability::ListCapabilitiesRequest {});
                api.enrich_metadata(&mut req).await?;
                let resp = client.list_capabilities(req).await.map_err(|s| {
                    FlareError::system(format!("ListCapabilities: {}", s.message()))
                })?;
                let out = resp
                    .into_inner()
                    .capabilities
                    .into_iter()
                    .map(|c| CapabilityDescriptorDto {
                        capability_id: c.capability_id,
                        plugin_id: c.plugin_id,
                        version: c.version,
                        scope: c.scope,
                        visibility: c.visibility,
                        permissions: c.permissions,
                        message_types: c.message_types,
                        timeout_ms: c.timeout_ms,
                        description: c.description,
                    })
                    .collect();
                Ok(out)
            })
            .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn list_capabilities(&self) -> Result<Vec<CapabilityDescriptorDto>> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn list_user_capabilities(
        &self,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>> {
        let api = self.clone();
        let tenant_id = tenant_id.map(str::to_string);
        let user_id = user_id.map(str::to_string);
        self.session_guard
            .run_with_user(move |session_user_id| async move {
                let tenant = api.effective_tenant_id(tenant_id.as_deref());
                let user = user_id
                    .or(session_user_id)
                    .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未连接"))?;
                let ch = api.connect().await?;
                let mut client = CapabilityServiceClient::new(ch);
                let mut req = tonic::Request::new(ListUserCapabilitiesRequest {
                    tenant_id: tenant,
                    user_id: user,
                });
                api.enrich_metadata(&mut req).await?;
                let resp = client.list_user_capabilities(req).await.map_err(|s| {
                    FlareError::system(format!("ListUserCapabilities: {}", s.message()))
                })?;
                let mut out = Vec::new();
                for g in resp.into_inner().grants {
                    let granted_at = g
                        .granted_at
                        .map(|t| prost_timestamp_to_rfc3339(&t))
                        .unwrap_or_default();
                    let expires_at = g.expires_at.as_ref().map(prost_timestamp_to_rfc3339);
                    let plan = if g.plan_code.is_empty() {
                        None
                    } else {
                        Some(g.plan_code)
                    };
                    let source = if g.source.is_empty() {
                        None
                    } else {
                        Some(g.source)
                    };
                    out.push(UserCapabilityGrantDto {
                        tenant_id: g.tenant_id,
                        user_id: g.user_id,
                        capability_id: g.capability_id,
                        granted_at,
                        expires_at,
                        plan_code: plan,
                        source,
                    });
                }
                Ok(out)
            })
            .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn list_user_capabilities(
        &self,
        _tenant_id: Option<&str>,
        _user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn dispatch(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        let api = self.clone();
        let capability_id = capability_id.to_string();
        let conversation_id = conversation_id.map(str::to_string);
        let tenant_id = tenant_id.map(str::to_string);
        let user_id = user_id.map(str::to_string);
        self.session_guard
            .run_with_user(move |session_user_id| async move {
                let tenant = api.effective_tenant_id(tenant_id.as_deref());
                let user = user_id
                    .or(session_user_id)
                    .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未连接"))?;
                let payload_json = if payload.is_null() {
                    String::new()
                } else {
                    serde_json::to_string(&payload)
                        .map_err(|e| FlareError::system(format!("capability payload json: {e}")))?
                };
                let ch = api.connect().await?;
                let mut client = CapabilityServiceClient::new(ch);
                let mut req = tonic::Request::new(DispatchCapabilityRequest {
                    capability_id,
                    tenant_id: tenant,
                    user_id: user,
                    conversation_id: conversation_id.unwrap_or_default(),
                    payload_json,
                    request_id: String::new(),
                });
                api.enrich_metadata(&mut req).await?;
                let resp = client
                    .dispatch(req)
                    .await
                    .map_err(|s| FlareError::system(format!("Dispatch: {}", s.message())))?;
                let r = resp
                    .into_inner()
                    .result
                    .ok_or_else(|| FlareError::system("Dispatch: empty result"))?;
                let data = if r.result_json.trim().is_empty() {
                    Value::Null
                } else {
                    serde_json::from_str(&r.result_json).unwrap_or(Value::Null)
                };
                let err = if r.error_message.is_empty() {
                    None
                } else {
                    Some(r.error_message)
                };
                Ok(CapabilityDispatchResult {
                    request_id: r.request_id,
                    success: r.success,
                    plugin_id: r.plugin_id,
                    capability_id: r.capability_id,
                    data,
                    error: err,
                })
            })
            .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn dispatch(
        &self,
        _capability_id: &str,
        _payload: Value,
        _conversation_id: Option<&str>,
        _tenant_id: Option<&str>,
        _user_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_start_audio(
        &self,
        conversation_id: &str,
        codec: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::CALL_AUDIO,
            rtc_ids::payload_start_audio(codec),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_start_video(
        &self,
        conversation_id: &str,
        codec: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::CALL_VIDEO,
            rtc_ids::payload_start_video(codec),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_accept(
        &self,
        conversation_id: &str,
        call_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::CALL_ACCEPT,
            rtc_ids::payload_call_id(call_id),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_end(
        &self,
        conversation_id: &str,
        call_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::CALL_END,
            rtc_ids::payload_call_id(call_id),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_reject(
        &self,
        conversation_id: &str,
        call_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::CALL_REJECT,
            rtc_ids::payload_call_id(call_id),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_join_room(
        &self,
        conversation_id: &str,
        call_id: &str,
        room_id: &str,
        role: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_JOIN_ROOM,
            rtc_ids::payload_sfu_join_room(call_id, room_id, role),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_leave_room(
        &self,
        conversation_id: &str,
        room_id: &str,
        peer_id: &str,
        session_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_LEAVE_ROOM,
            rtc_ids::payload_sfu_leave_room(room_id, peer_id, session_id),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_handle_sdp_offer(
        &self,
        conversation_id: &str,
        room_id: &str,
        peer_id: &str,
        sdp_offer: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_HANDLE_SDP_OFFER,
            rtc_ids::payload_sfu_handle_sdp_offer(room_id, peer_id, sdp_offer),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_handle_sdp_answer(
        &self,
        conversation_id: &str,
        room_id: &str,
        peer_id: &str,
        sdp_answer: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_HANDLE_SDP_ANSWER,
            rtc_ids::payload_sfu_handle_sdp_answer(room_id, peer_id, sdp_answer),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_add_ice_candidate(
        &self,
        conversation_id: &str,
        room_id: &str,
        peer_id: &str,
        candidate_json: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_ADD_ICE_CANDIDATE,
            rtc_ids::payload_sfu_add_ice_candidate(room_id, peer_id, candidate_json),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_set_subscription(
        &self,
        request: RtcSfuSubscriptionRequest,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_SET_SUBSCRIPTION,
            rtc_ids::payload_sfu_set_subscription(
                &request.room_id,
                &request.subscriber_peer_id,
                &request.track_id,
                request.enable,
                request.media.as_deref(),
                request.preferred_layer.as_deref(),
                request.priority,
            ),
            Some(&request.conversation_id),
            request.tenant_id.as_deref(),
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn rtc_sfu_get_room_state(
        &self,
        conversation_id: &str,
        room_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch(
            rtc_ids::SFU_GET_ROOM_STATE,
            rtc_ids::payload_sfu_get_room_state(room_id),
            Some(conversation_id),
            tenant_id,
            None,
        )
        .await?
        .fail_if_unsuccessful()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn grant_user_capability(
        &self,
        tenant_id: &str,
        user_id: &str,
        capability_id: &str,
        expires_at_rfc3339: Option<&str>,
        plan_code: Option<&str>,
        source: Option<&str>,
    ) -> Result<()> {
        let api = self.clone();
        let tenant_id = tenant_id.to_string();
        let user_id = user_id.to_string();
        let capability_id = capability_id.to_string();
        let expires_at_rfc3339 = expires_at_rfc3339.unwrap_or("").to_string();
        let plan_code = plan_code.unwrap_or("").to_string();
        let source = source.unwrap_or("").to_string();
        self.session_guard
            .run(async move {
                let ch = api.connect().await?;
                let mut client = CapabilityServiceClient::new(ch);
                let mut req = tonic::Request::new(GrantUserCapabilityRequest {
                    tenant_id,
                    user_id,
                    capability_id,
                    expires_at_rfc3339,
                    plan_code,
                    source,
                });
                api.enrich_metadata(&mut req).await?;
                client.grant_user_capability(req).await.map_err(|s| {
                    FlareError::system(format!("GrantUserCapability: {}", s.message()))
                })?;
                Ok(())
            })
            .await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn revoke_user_capability(
        &self,
        tenant_id: &str,
        user_id: &str,
        capability_id: &str,
    ) -> Result<()> {
        let api = self.clone();
        let tenant_id = tenant_id.to_string();
        let user_id = user_id.to_string();
        let capability_id = capability_id.to_string();
        self.session_guard
            .run(async move {
                let ch = api.connect().await?;
                let mut client = CapabilityServiceClient::new(ch);
                let mut req = tonic::Request::new(RevokeUserCapabilityRequest {
                    tenant_id,
                    user_id,
                    capability_id,
                });
                api.enrich_metadata(&mut req).await?;
                client.revoke_user_capability(req).await.map_err(|s| {
                    FlareError::system(format!("RevokeUserCapability: {}", s.message()))
                })?;
                Ok(())
            })
            .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn grant_user_capability(
        &self,
        _tenant_id: &str,
        _user_id: &str,
        _capability_id: &str,
        _expires_at_rfc3339: Option<&str>,
        _plan_code: Option<&str>,
        _source: Option<&str>,
    ) -> Result<()> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn revoke_user_capability(
        &self,
        _tenant_id: &str,
        _user_id: &str,
        _capability_id: &str,
    ) -> Result<()> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_start_audio(
        &self,
        _conversation_id: &str,
        _codec: Option<&str>,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_start_video(
        &self,
        _conversation_id: &str,
        _codec: Option<&str>,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_accept(
        &self,
        _conversation_id: &str,
        _call_id: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_end(
        &self,
        _conversation_id: &str,
        _call_id: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_reject(
        &self,
        _conversation_id: &str,
        _call_id: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_sfu_join_room(
        &self,
        _conversation_id: &str,
        _call_id: &str,
        _room_id: &str,
        _role: Option<&str>,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_sfu_leave_room(
        &self,
        _conversation_id: &str,
        _room_id: &str,
        _peer_id: &str,
        _session_id: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_sfu_handle_sdp_offer(
        &self,
        _conversation_id: &str,
        _room_id: &str,
        _peer_id: &str,
        _sdp_offer: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_sfu_handle_sdp_answer(
        &self,
        _conversation_id: &str,
        _room_id: &str,
        _peer_id: &str,
        _sdp_answer: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn rtc_sfu_add_ice_candidate(
        &self,
        _conversation_id: &str,
        _room_id: &str,
        _peer_id: &str,
        _candidate_json: &str,
        _tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "sdk.capability.wasm_not_supported",
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn prost_timestamp_to_rfc3339(t: &prost_types::Timestamp) -> String {
    use chrono::{TimeZone, Utc};
    let dt = Utc
        .timestamp_opt(t.seconds, t.nanos as u32)
        .single()
        .unwrap_or_else(Utc::now);
    dt.to_rfc3339()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDescriptorDto {
    pub capability_id: String,
    pub plugin_id: String,
    pub version: String,
    pub scope: String,
    pub visibility: String,
    pub permissions: Vec<String>,
    pub message_types: Vec<String>,
    pub timeout_ms: u64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCapabilityGrantDto {
    pub tenant_id: String,
    pub user_id: String,
    pub capability_id: String,
    pub granted_at: String,
    pub expires_at: Option<String>,
    pub plan_code: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityDispatchResult {
    pub request_id: String,
    pub success: bool,
    pub plugin_id: String,
    pub capability_id: String,
    pub data: Value,
    pub error: Option<String>,
}

impl CapabilityDispatchResult {
    /// gRPC 已返回但业务层 `success == false`（SFU/插件不可用或拒绝）时转为 [`FlareError`]，便于宿主直接提示用户。
    pub fn fail_if_unsuccessful(self) -> Result<Self> {
        if self.success {
            return Ok(self);
        }
        let msg = self
            .error
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| "capability service returned failure".to_string());
        Err(FlareError::localized(ErrorCode::ServiceUnavailable, msg))
    }
}
