//! Unified contract API invoke — used by Tauri `sdk_invoke_json`, C `flare_sdk_invoke_json`, and wasm.

use std::borrow::Cow;

use serde_json::Value;

use crate::generated::dispatch::{capability, conversation, media, message, message_build};
use crate::operation::normalize_operation;
use crate::{BindingResponse, binding_operation_not_supported};
use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::IMClient;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi,
};
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

pub async fn invoke_api_id_json(
    session: &impl InvokeSession,
    api_id: &str,
    request_json: &str,
) -> Result<BindingResponse> {
    let normalized = normalize_operation_json(api_id, request_json)?;
    invoke_normalized_json(session, &normalized.name, normalized.request.as_ref()).await
}

struct NormalizedJsonOperation<'a> {
    name: String,
    request: Cow<'a, str>,
}

fn normalize_operation_json<'a>(
    operation: &str,
    request_json: &'a str,
) -> Result<NormalizedJsonOperation<'a>> {
    if operation == "message_builder.dispatch" || operation.starts_with("message_builder.create_") {
        return normalize_operation_json_with_request(operation, request_json);
    }

    match operation {
        "rich_doc_v2.create_message" => {
            normalize_operation_json_with_request(operation, request_json)
        }
        op if op.starts_with("rtc.") => normalize_operation_json_with_request(op, request_json),
        "rich_doc_v2.edit_message" => Ok(NormalizedJsonOperation {
            name: "message.edit_rich_doc_by_message_id".to_string(),
            request: Cow::Borrowed(request_json),
        }),
        _ => Ok(NormalizedJsonOperation {
            name: operation.to_string(),
            request: Cow::Borrowed(request_json),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_im_core_sdk::client::IMClient;
    use flare_im_core_sdk::client::api::{
        CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi,
    };
    use std::sync::Arc;

    struct DirectOnlySession {
        client: IMClient,
    }

    impl DirectOnlySession {
        fn new() -> Self {
            Self {
                client: IMClient::new(),
            }
        }
    }

    impl InvokeSession for DirectOnlySession {
        fn client(&self) -> IMClient {
            self.client.clone()
        }

        async fn message_api(&self) -> Result<MessageApi> {
            unreachable!("direct route tests must not require message api")
        }

        async fn message_build_api(&self) -> Result<Arc<MessageBuildApi>> {
            unreachable!("direct route tests must not require message build api")
        }

        async fn conversation_api(&self) -> Result<ConversationApi> {
            unreachable!("direct route tests must not require conversation api")
        }

        async fn media_api(&self) -> Result<Arc<MediaApi>> {
            unreachable!("direct route tests must not require media api")
        }

        async fn capability_api(&self) -> Result<Arc<CapabilityApi>> {
            unreachable!("direct route tests must not require capability api")
        }

        async fn after_disconnect(&self) {}
    }

    #[test]
    fn normalizes_message_builder_typed_dispatch_json_to_message_build() {
        let normalized = normalize_operation_json(
            "message_builder.dispatch",
            r#"{"op":"create_text","conversationId":"c1","text":"hello"}"#,
        )
        .expect("message builder dispatch should normalize");

        assert_eq!(normalized.name, "message.build");
        assert_eq!(
            normalized.request.as_ref(),
            r#"{"conversationId":"c1","op":"create_text","text":"hello"}"#
        );
    }

    #[tokio::test]
    async fn direct_connection_state_returns_canonical_enum_string() {
        let session = DirectOnlySession::new();

        let response = invoke_api_id_json(&session, "connection.get_state", "{}")
            .await
            .expect("connection state");
        let value = binding_response_to_value(response);

        assert_eq!(value, serde_json::json!("disconnected"));
    }

    #[tokio::test]
    async fn direct_boolean_routes_return_bare_booleans() {
        let session = DirectOnlySession::new();

        let connected = binding_response_to_value(
            invoke_api_id_json(&session, "sdk.is_connected", "{}")
                .await
                .expect("is connected"),
        );
        let active = binding_response_to_value(
            invoke_api_id_json(&session, "sdk.session_active", "{}")
                .await
                .expect("session active"),
        );

        assert_eq!(connected, serde_json::json!(false));
        assert_eq!(active, serde_json::json!(false));
    }

    #[tokio::test]
    async fn direct_rich_doc_normalization_runs_without_module_api() {
        let session = DirectOnlySession::new();
        let doc_json = r#"{
            "type": "doc",
            "version": 2,
            "children": [{
                "type": "paragraph",
                "children": [{"type": "text", "text": "hello flare"}]
            }]
        }"#;
        let request = serde_json::json!({
            "docJson": doc_json,
        });

        let response = invoke_api_id_json(
            &session,
            "rich_doc_v2.normalize_from_doc_json",
            &request.to_string(),
        )
        .await
        .expect("rich doc direct normalize");
        let value = binding_response_to_value(response);

        assert_eq!(value["plainText"], "hello flare");
        assert_eq!(value["contentSchema"], "rich_doc");
    }
}

fn normalize_operation_json_with_request<'a>(
    operation: &str,
    request_json: &'a str,
) -> Result<NormalizedJsonOperation<'a>> {
    let request = crate::dispatch_support::dispatch_params_from_json(request_json)?;
    let normalized = normalize_operation(operation, request);
    let request = serde_json::to_string(&normalized.request)
        .map_err(|e| crate::binding_invalid_parameter(format!("invalid request JSON: {e}")))?;
    Ok(NormalizedJsonOperation {
        name: normalized.name,
        request: Cow::Owned(request),
    })
}

async fn invoke_normalized(
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
        name if name.starts_with("view.") => {
            let op = name.strip_prefix("view.").unwrap_or(name);
            let api = session.client().view_async().await?;
            dispatch_view(&api, op, request).await
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

pub async fn invoke_normalized_json(
    session: &impl InvokeSession,
    route: &str,
    request_json: &str,
) -> Result<BindingResponse> {
    if crate::generated::direct_invoke::is_direct_invoke_route(route) {
        return crate::generated::direct_invoke::dispatch_direct_json(session, route, request_json)
            .await;
    }

    match route {
        "message.build" => {
            let api = session.message_build_api().await?;
            message_build::dispatch_message_build_json(&api, request_json).await
        }
        name if name.starts_with("message.") => {
            let op = name.strip_prefix("message.").unwrap_or(name);
            let api = session.message_api().await?;
            message::dispatch_message_json(&api, op, request_json).await
        }
        name if name.starts_with("conversation.") => {
            let op = name.strip_prefix("conversation.").unwrap_or(name);
            let api = session.conversation_api().await?;
            conversation::dispatch_conversation_json(&api, op, request_json).await
        }
        name if name.starts_with("view.") => {
            let op = name.strip_prefix("view.").unwrap_or(name);
            let api = session.client().view_async().await?;
            let request = crate::dispatch_support::dispatch_params_from_json(request_json)?;
            dispatch_view(&api, op, request).await
        }
        name if name.starts_with("media.") => {
            let op = name.strip_prefix("media.").unwrap_or(name);
            let api = session.media_api().await?;
            media::dispatch_media_json(&api, op, request_json).await
        }
        "capability.list" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability_json(
                &api,
                &session.client(),
                "capability_list",
                request_json,
            )
            .await
        }
        "capability.list_user" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability_json(
                &api,
                &session.client(),
                "capability_list_user",
                request_json,
            )
            .await
        }
        "capability.dispatch" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability_json(
                &api,
                &session.client(),
                "capability_dispatch",
                request_json,
            )
            .await
        }
        "capability.grant" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability_json(
                &api,
                &session.client(),
                "capability_grant",
                request_json,
            )
            .await
        }
        "capability.revoke" => {
            let api = session.capability_api().await?;
            capability::dispatch_capability_json(
                &api,
                &session.client(),
                "capability_revoke",
                request_json,
            )
            .await
        }
        name if name.starts_with("capability.") => {
            let api = session.capability_api().await?;
            capability::dispatch_capability_json(
                &api,
                &session.client(),
                name.strip_prefix("capability.").unwrap_or(name),
                request_json,
            )
            .await
        }
        _ => {
            let request = crate::dispatch_support::dispatch_params_from_json(request_json)?;
            invoke_normalized(session, route, request).await
        }
    }
}

async fn dispatch_view(
    api: &Arc<flare_im_core_sdk::client::api::ViewApi>,
    operation: &str,
    request: Value,
) -> Result<BindingResponse> {
    match operation {
        "timeline.open" => {
            let request = crate::dispatch_support::from_value::<
                flare_im_core_sdk::model::OpenTimelineViewRequest,
            >(request, "open timeline view request")?;
            crate::dispatch_support::json(api.open_timeline(request).await?)
        }
        "timeline.load_older" => {
            let request = crate::dispatch_support::from_value::<
                flare_im_core_sdk::model::LoadOlderTimelineViewRequest,
            >(request, "load older timeline view request")?;
            crate::dispatch_support::json(api.load_older_timeline(request).await?)
        }
        "conversation_list.open" => {
            let request = crate::dispatch_support::from_value::<
                flare_im_core_sdk::model::OpenConversationListViewRequest,
            >(request, "open conversation list view request")?;
            crate::dispatch_support::json(api.open_conversation_list(request).await?)
        }
        "timeline.close" | "conversation_list.close" | "close" => {
            let request = crate::dispatch_support::from_value::<
                flare_im_core_sdk::model::CloseViewRequest,
            >(request, "close view request")?;
            crate::dispatch_support::json(api.close(request).await?)
        }
        _ => Err(binding_operation_not_supported(&format!(
            "view.{operation}"
        ))),
    }
}

pub fn binding_response_to_value(response: BindingResponse) -> Value {
    if response.is_unit {
        Value::Null
    } else {
        response.payload
    }
}
