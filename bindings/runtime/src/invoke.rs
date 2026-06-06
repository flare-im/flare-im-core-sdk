//! Unified contract API invoke — used by Tauri `sdk_invoke`, C `flare_sdk_invoke_json`, and wasm.

use serde_json::Value;

use crate::generated::dispatch::{capability, conversation, media, message, message_build};
use crate::operation::normalize_operation;
use crate::{BindingResponse, binding_operation_not_supported};
use flare_im_core_sdk::client::IMClient;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi,
};
use flare_im_core_sdk::shared::error::Result;
use std::future::Future;
use std::sync::Arc;

/// Session APIs required for contract invoke (implemented by Tauri [`SdkState`] and C [`SdkInstance`]).
pub trait InvokeSession: Send + Sync {
    fn client(&self) -> IMClient;
    fn message_api(&self) -> impl Future<Output = Result<MessageApi>> + Send;
    fn message_build_api(&self) -> impl Future<Output = Result<Arc<MessageBuildApi>>> + Send;
    fn conversation_api(&self) -> impl Future<Output = Result<ConversationApi>> + Send;
    fn media_api(&self) -> impl Future<Output = Result<Arc<MediaApi>>> + Send;
    fn capability_api(&self) -> impl Future<Output = Result<Arc<CapabilityApi>>> + Send;

    /// Clears binding-layer API caches after [`IMClient::disconnect`].
    fn after_disconnect(&self) -> impl Future<Output = ()> + Send;
}

pub async fn invoke_api_id(
    session: &impl InvokeSession,
    api_id: &str,
    request: Value,
) -> Result<BindingResponse> {
    let normalized = normalize_operation(api_id, request);
    invoke_normalized(session, &normalized.name, normalized.request).await
}

pub async fn invoke_normalized(
    session: &impl InvokeSession,
    route: &str,
    request: Value,
) -> Result<BindingResponse> {
    if crate::generated::direct_invoke::is_direct_invoke_route(route) {
        return crate::generated::direct_invoke::dispatch_direct(session, route, &request).await;
    }

    match route {
        "message.build" => {
            let api = session.message_build_api().await?;
            message_build::dispatch_message_build(&api, request).await
        }
        name if name.starts_with("message.") => {
            let op = name.strip_prefix("message.").unwrap_or(name);
            let api = session.message_api().await?;
            message::dispatch_message(&api, op, request).await
        }
        name if name.starts_with("conversation.") => {
            let op = name.strip_prefix("conversation.").unwrap_or(name);
            let api = session.conversation_api().await?;
            conversation::dispatch_conversation(&api, op, request).await
        }
        name if name.starts_with("media.") => {
            let op = name.strip_prefix("media.").unwrap_or(name);
            let api = session.media_api().await?;
            media::dispatch_media(&api, op, request).await
        }
        "capability.list" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(&api, &session.client(), "capability_list", request)
                .await
        }
        "capability.list_user" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(
                &api,
                &session.client(),
                "capability_list_user",
                request,
            )
            .await
        }
        "capability.dispatch" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(&api, &session.client(), "capability_dispatch", request)
                .await
        }
        "capability.grant" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(&api, &session.client(), "capability_grant", request)
                .await
        }
        "capability.revoke" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(&api, &session.client(), "capability_revoke", request)
                .await
        }
        "capability.send_call_signal" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(&api, &session.client(), "send_call_signal", request)
                .await
        }
        name if name.starts_with("capability.") => {
            let api = session.capability_api().await?;
            capability::dispatch_capability(
                &api,
                &session.client(),
                name.strip_prefix("capability.").unwrap_or(name),
                request,
            )
            .await
        }
        _ => Err(binding_operation_not_supported(route)),
    }
}

pub fn binding_response_to_value(response: BindingResponse) -> Value {
    if response.is_unit {
        Value::Null
    } else {
        response.payload
    }
}
