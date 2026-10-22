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

use super::session_guard::SessionGuard;
#[cfg(not(target_arch = "wasm32"))]
use crate::extension::capability::rtc_ids;
use crate::infrastructure::transport::http::http_client::HttpRequestContext;
use crate::infrastructure::transport::http::{HttpApiResponse, HttpClient, unwrap_api_response};
use crate::shared::error::{ErrorCode, FlareError, Result};

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "capability_ui_tests.rs"]
mod ui_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "capability_ui_v2_tests.rs"]
mod ui_v2_tests;

#[derive(Clone)]
pub struct CapabilityApi {
    pub(super) plugin_commands: Option<Arc<dyn crate::domain::PluginCommandStore>>,
    /// Local message store and bus for plugin card state (`messageId` requests).
    messages: Option<Arc<dyn crate::domain::MessageStore>>,
    bus: Option<crate::kernel::event::EventBus>,
    /// Background read of a presented card's shared layer when its message has none.
    plugin_state_refetch: Option<Arc<crate::application::services::PluginStateRefetch>>,
    http: Option<HttpClient>,
    pub(super) session_guard: SessionGuard,
    #[cfg(not(target_arch = "wasm32"))]
    endpoint: String,
    #[cfg(not(target_arch = "wasm32"))]
    channel: Arc<Mutex<Option<Channel>>>,
    #[cfg(not(target_arch = "wasm32"))]
    current_user_id: Arc<RwLock<String>>,
    pub(super) default_tenant_id: String,
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
        let _ = (grpc_endpoint.into(), http_request_context);
        Self {
            plugin_commands: None,
            messages: None,
            bus: None,
            plugin_state_refetch: None,
            http: None,
            session_guard: SessionGuard::new(current_user_id.clone(), "capability"),
            #[cfg(not(target_arch = "wasm32"))]
            endpoint: grpc_endpoint.into(),
            #[cfg(not(target_arch = "wasm32"))]
            channel: Arc::new(Mutex::new(None)),
            #[cfg(not(target_arch = "wasm32"))]
            current_user_id,
            default_tenant_id: crate::shared::util::normalize_tenant_id(default_tenant_id.into()),
            #[cfg(not(target_arch = "wasm32"))]
            http_request_context,
        }
    }
    pub fn with_plugin_command_store(
        mut self,
        store: Option<Arc<dyn crate::domain::PluginCommandStore>>,
    ) -> Self {
        self.plugin_commands = store;
        self
    }
    /// Enables plugin card state: present overlays the state stored on `messageId`,
    /// invoke writes a successful action's `data.messageState` back and publishes
    /// `MessageEvent::PluginStateChanged`.
    pub fn with_message_state(
        mut self,
        messages: Arc<dyn crate::domain::MessageStore>,
        bus: crate::kernel::event::EventBus,
    ) -> Self {
        self.messages = Some(messages);
        self.bus = Some(bus);
        self
    }
    /// Heals a card cached before the client knew shared state: presenting a card
    /// whose message has no shared layer reads that message back once
    /// ([`crate::application::services::PluginStateRefetch`]).
    pub(crate) fn with_plugin_state_refetch(
        mut self,
        refetch: Arc<crate::application::services::PluginStateRefetch>,
    ) -> Self {
        self.plugin_state_refetch = Some(refetch);
        self
    }
    pub fn with_http_transport(mut self, http: HttpClient) -> Self {
        self.http = Some(http);
        self
    }
    async fn dispatch_http(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let expected_request_id = request_id.clone();
        let http = self.http.as_ref().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::OperationNotSupported,
                "sdk.capability.http_unavailable",
            )
        })?;
        let result: CapabilityDispatchResult = self
            .session_guard
            .run_with_user(|identity| async move {
                let user = identity.ok_or_else(|| {
                    FlareError::localized(
                        ErrorCode::NotConnected,
                        "sdk.capability.session_required",
                    )
                })?;
                if tenant_id.is_some_and(|tenant| tenant != self.default_tenant_id)
                    || user_id.is_some_and(|expected| expected != user)
                {
                    return Err(FlareError::localized(
                        ErrorCode::InvalidParameter,
                        "sdk.capability.identity_override_denied",
                    ));
                }
                let body: HttpApiResponse<CapabilityDispatchResult> = http
                    .post(
                        "/api/v1/capabilities/dispatch",
                        &serde_json::json!({
                            "capabilityId": capability_id, "payload": payload,
                            "conversationId": conversation_id, "requestId": request_id,
                            "expectedTenantId": self.default_tenant_id, "expectedUserId": user,
                        }),
                    )
                    .await?;
                unwrap_api_response(body, "capability dispatch")
            })
            .await?;
        if result.request_id != expected_request_id || result.capability_id != capability_id {
            return Err(FlareError::system("capability response identity mismatch"));
        }
        Ok(result)
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

    pub async fn resolve_plugin_message(&self, request: Value) -> Result<Value> {
        self.session_guard
            .run(async {
                crate::extension::capability::ui::resolve(&self.default_tenant_id, request)
            })
            .await
    }
    pub async fn plugin_ui_action(&self, request: Value) -> Result<Value> {
        self.session_guard
            .run(async {
                crate::extension::capability::ui::action(&self.default_tenant_id, request)
            })
            .await
    }
    /// Native/Web hosts render the same Rust-evaluated presentation. No positive
    /// grant cache is kept between calls, and logout fences pending work.
    ///
    /// With `messageId`, the card is resolved over the state layers stored on
    /// that message, `payload ← shared ← local`: the shared layer is the entity
    /// state Core published (events and history), the local layer the viewer's
    /// own `data.messageState`. `hiddenWhen`, fields and blocks see the merged
    /// data. The result is the v2 view (`blocks`, `inputs`, `detail` when the
    /// document declares them) plus `data` (the merged data, only when
    /// `available`), `stateVersion` (local layer, 0 when none) and
    /// `sharedStateVersion` (shared layer, 0 when none).
    ///
    /// A card of an entity (`entityPath`) whose stored message has no shared
    /// layer, such as one cached before the client knew shared state, is read
    /// back from the server once in the background; a layer found there is
    /// stored and announced with `PluginStateChanged`, so the host presents
    /// the card again.
    pub async fn present_plugin_message(&self, request: Value) -> Result<Value> {
        use crate::extension::capability::ui;
        let request = ui::parse_present(request)?;
        self.session_guard
            .run_with_user(|viewer| async move {
                let (_, catalog) = self.fetch_plugin_ui_catalog().await?;
                let stored = match request.message_id.as_deref() {
                    Some(message_id) => {
                        self.stored_plugin_message(message_id, &request.message)
                            .await
                    }
                    None => None,
                };
                let kind = request
                    .kind
                    .unwrap_or(flare_plugin_protocol::manifest::UiContributionKind::MessageContent);
                let entity = ui::contribution_of(&catalog, &request.message, Some(kind))
                    .and_then(|contribution| contribution.entity_of(&request.message.payload));
                let state = ui::CardState::of(stored.as_ref(), entity.as_deref());
                let viewer = viewer.unwrap_or_default();
                if let (Some(refetch), Some(stored), Some(entity)) =
                    (&self.plugin_state_refetch, &stored, entity.as_deref())
                {
                    refetch.on_present(stored, entity, &viewer);
                }
                let mut view = ui::present_card(
                    &catalog,
                    &request.message,
                    &state,
                    &ui::CardPresentation {
                        tenant: &self.default_tenant_id,
                        platform: request.platform,
                        kind,
                        locale: &request.locale,
                        viewer: &viewer,
                    },
                )?;
                if let Some(view) = view.as_object_mut() {
                    view.insert(
                        "stateVersion".into(),
                        Value::from(state.local.as_ref().map_or(0, |state| state.version)),
                    );
                    view.insert(
                        "sharedStateVersion".into(),
                        Value::from(state.shared.as_ref().map_or(0, |state| state.version)),
                    );
                }
                Ok(view)
            })
            .await
    }
    /// The stored message for `message_id` (server id or client message id), only
    /// when it carries the same plugin card. Read failures degrade to no state.
    async fn stored_plugin_message(
        &self,
        message_id: &str,
        message: &flare_plugin_protocol::ui::PluginMessage,
    ) -> Option<crate::model::IMMessage> {
        let store = self.messages.as_ref()?;
        let stored = match store.get(message_id).await {
            Ok(Some(stored)) => Some(stored),
            Ok(None) => store.get_by_client_msg_id(message_id).await.unwrap_or_else(|error| {
                tracing::warn!(target = "flare_sdk.plugin", %error, "read plugin message failed");
                None
            }),
            Err(error) => {
                tracing::warn!(target = "flare_sdk.plugin", %error, "read plugin message failed");
                None
            }
        }?;
        stored
            .plugin_message()
            .is_some_and(|envelope| {
                envelope.plugin_id == message.plugin_id
                    && envelope.contribution_id == message.contribution_id
            })
            .then_some(stored)
    }
    /// Best effort, after a successful action from a message card:
    /// - `data.sharedState` of the card's own entity is applied at once as the
    ///   shared layer (newer versions only), so the caller does not wait for
    ///   Core's broadcast; that broadcast of the same version is then a duplicate.
    /// - `data.messageState` is validated and stored as the local layer by version.
    ///
    /// One `PluginStateChanged` announces whatever changed (its version is the
    /// local layer's when that changed, the shared layer's otherwise). Failures
    /// are logged only; they never change the action result.
    async fn write_plugin_message_state(
        &self,
        message_id: &str,
        message: &flare_plugin_protocol::ui::PluginMessage,
        entity: Option<&str>,
        response: &CapabilityDispatchResult,
    ) {
        use crate::domain::OperationApplyResult;
        if !response.success {
            return;
        }
        let local =
            response.data.get("messageState").and_then(|raw| {
                match serde_json::from_value::<flare_plugin_protocol::ui::PluginMessageState>(
                    raw.clone(),
                ) {
                    Ok(state) if state.validate().is_ok() => Some(state),
                    _ => {
                        tracing::warn!(
                            target = "flare_sdk.plugin",
                            plugin_id = %message.plugin_id,
                            "plugin returned an invalid message state"
                        );
                        None
                    }
                }
            });
        let shared = match flare_plugin_protocol::state::SharedState::from_result(&response.data) {
            Ok(Some(state)) if entity == Some(state.entity.as_str()) => Some(state),
            Ok(None) => None,
            Ok(Some(_)) | Err(_) => {
                tracing::warn!(
                    target = "flare_sdk.plugin",
                    plugin_id = %message.plugin_id,
                    "plugin returned a shared state that is invalid or not this card's entity"
                );
                None
            }
        };
        if local.is_none() && shared.is_none() {
            return;
        }
        let (Some(store), Some(stored)) = (
            self.messages.as_ref(),
            self.stored_plugin_message(message_id, message).await,
        ) else {
            tracing::debug!(
                target = "flare_sdk.plugin",
                "plugin message state skipped: no matching local message"
            );
            return;
        };
        let mut changed = None;
        if let Some(state) = &shared {
            match store.apply_plugin_shared_state(message_id, state).await {
                Ok(OperationApplyResult::Applied) => changed = Some(state.version),
                Ok(result) => tracing::debug!(
                    target = "flare_sdk.plugin",
                    ?result,
                    version = state.version,
                    "plugin shared state not applied"
                ),
                Err(error) => tracing::warn!(
                    target = "flare_sdk.plugin",
                    %error,
                    "write plugin shared state failed"
                ),
            }
        }
        if let Some(state) = &local {
            match store.apply_plugin_message_state(message_id, state).await {
                Ok(OperationApplyResult::Applied) => changed = Some(state.version),
                Ok(result) => tracing::debug!(
                    target = "flare_sdk.plugin",
                    ?result,
                    version = state.version,
                    "plugin message state not applied"
                ),
                Err(error) => tracing::warn!(
                    target = "flare_sdk.plugin",
                    %error,
                    "write plugin message state failed"
                ),
            }
        }
        if let (Some(version), Some(bus)) = (changed, &self.bus) {
            let message_id = if stored.server_id.trim().is_empty() {
                stored.client_msg_id
            } else {
                stored.server_id
            };
            bus.publish(crate::kernel::event::SdkEvent::Message(
                crate::kernel::event::MessageEvent::PluginStateChanged {
                    conversation_id: stored.conversation_id,
                    message_id,
                    version,
                },
            ));
        }
    }
    /// Refresh authorization, bind declared action input, then dispatch through
    /// the normal capability policy. Native/page targets return an explicit
    /// host instruction; only the application's allowlisted bridge executes it.
    ///
    /// The input is bound over the merged card data. An action with a `form`
    /// takes `formValues`, validated first: problems come back as
    /// `{kind: "formErrors", errors: [{input, issue}]}` and nothing is sent. An
    /// action owned by a choices block takes `selection` (option indices).
    /// With `messageId`, a successful operation's `data.sharedState` (this
    /// card's entity) and `data.messageState` are stored on that message; calls
    /// not made from a message ignore them.
    pub async fn invoke_plugin_action(&self, request: Value) -> Result<Value> {
        use crate::extension::capability::ui;
        use flare_plugin_protocol::ui::UiActionTarget;
        let request = ui::parse_invoke(request)?;
        self.session_guard
            .run_with_user(|user_id| async move {
                let (_, catalog) = self.fetch_plugin_ui_catalog().await?;
                let stored = match request.message_id.as_deref() {
                    Some(message_id) => {
                        self.stored_plugin_message(message_id, &request.message)
                            .await
                    }
                    None => None,
                };
                match ui::prepare_action(
                    &self.default_tenant_id,
                    &catalog,
                    &request,
                    stored.as_ref(),
                )? {
                    ui::PreparedAction::FormErrors(errors) => {
                        Ok(serde_json::json!({"kind":"formErrors","errors":errors}))
                    }
                    ui::PreparedAction::Ready {
                        target: UiActionTarget::Operation { operation },
                        payload,
                        entity,
                    } => {
                        let response = self
                            .dispatch(
                                &operation,
                                payload,
                                request.conversation_id.as_deref(),
                                Some(&self.default_tenant_id),
                                user_id.as_deref(),
                            )
                            .await?;
                        if let Some(message_id) = request.message_id.as_deref() {
                            self.write_plugin_message_state(
                                message_id,
                                &request.message,
                                entity.as_deref(),
                                &response,
                            )
                            .await;
                        }
                        Ok(serde_json::json!({"kind":"operation","response":response}))
                    }
                    ui::PreparedAction::Ready {
                        target, payload, ..
                    } => Ok(serde_json::json!({"kind":"host","target":target,"payload":payload})),
                }
            })
            .await
    }
    /// "View all" of a message card: invokes the contribution's detail operation
    /// with its input bound over the merged card data (`messageId` adds the
    /// stored state layers), then resolves the detail document against the
    /// result `data`. Returns that v2 view (`available`, `title`, `fields`,
    /// `actions`, `blocks?`, `inputs?`, and `nextCursor` while a paged detail
    /// has more). A plugin failure is an error; a card without a detail, or one
    /// the catalog no longer offers, is `sdk.plugin.ui_action_unavailable`.
    ///
    /// Paging: the next page is requested with `cursor` (the view's
    /// `nextCursor`) and `previous` (that view, as returned); the operation
    /// gets the first page's input plus `cursor`, and the page comes back
    /// appended to `previous` (entries continue in order, everything else stays
    /// as on the first page, `nextCursor` is the new page's). Without
    /// `previous` only the page is returned. A `cursor` on a detail that does
    /// not page, or a malformed one, is an invalid request.
    ///
    /// Refresh: `refresh` with `previous` (the shown view) re-reads the detail
    /// from its first page and follows the cursors until the entries are at
    /// least as many as `previous` showed, there is no next page, or
    /// [`ui::MAX_DETAIL_REFRESH_PAGES`] pages were read, so a host that
    /// re-reads a sheet after a state change keeps the pages it had loaded.
    /// The merged view replaces `previous`; any page failing fails the call,
    /// and the host keeps showing `previous`.
    pub async fn plugin_detail(&self, request: Value) -> Result<Value> {
        use crate::extension::capability::ui;
        let mut request = ui::parse_detail(request)?;
        let previous = request.previous.take();
        self.session_guard
            .run_with_user(|user_id| async move {
                let (_, catalog) = self.fetch_plugin_ui_catalog().await?;
                let stored = match request.message_id.as_deref() {
                    Some(message_id) => {
                        self.stored_plugin_message(message_id, &request.message)
                            .await
                    }
                    None => None,
                };
                let detail = ui::prepare_detail(
                    &self.default_tenant_id,
                    &catalog,
                    &request,
                    stored.as_ref(),
                )?;
                // A malformed cursor is refused before anything is sent.
                let input = detail.input(request.cursor.as_deref())?;
                let user_id = user_id.as_deref();
                let mut view = self
                    .detail_page(&catalog, &request, &detail, input, user_id)
                    .await?;
                match previous {
                    Some(previous) if request.refresh => {
                        let wanted = ui::detail_entries(&previous);
                        let mut pages = 1;
                        while let Some(cursor) =
                            ui::detail_refresh_continues(&view, wanted, pages).map(str::to_owned)
                        {
                            let input = detail.input(Some(&cursor))?;
                            let page = self
                                .detail_page(&catalog, &request, &detail, input, user_id)
                                .await?;
                            view.append_page(page);
                            pages += 1;
                        }
                    }
                    Some(mut previous) => {
                        previous.append_page(view);
                        view = previous;
                    }
                    None => {}
                }
                ui::detail_view(&view)
            })
            .await
    }
    /// One page of a card's detail: the detail operation with `input`, its
    /// result resolved against the detail document.
    async fn detail_page(
        &self,
        catalog: &flare_plugin_protocol::ui::UiCatalog,
        request: &crate::extension::capability::ui::DetailRequest,
        detail: &crate::extension::capability::ui::PreparedDetail,
        input: Value,
        user_id: Option<&str>,
    ) -> Result<flare_plugin_protocol::ui::ResolvedPluginView> {
        use crate::extension::capability::ui;
        let response = self
            .dispatch(
                detail.operation(),
                input,
                request.conversation_id.as_deref(),
                Some(&self.default_tenant_id),
                user_id,
            )
            .await?
            .fail_if_unsuccessful()?;
        Ok(ui::detail_page(
            catalog,
            &request.message,
            &response.data,
            &ui::CardPresentation {
                tenant: &self.default_tenant_id,
                platform: request.platform,
                kind: flare_plugin_protocol::manifest::UiContributionKind::MessageContent,
                locale: &request.locale,
                viewer: user_id.unwrap_or_default(),
            },
        ))
    }
    /// Whether a plugin card offers "forward": only when the current catalog's
    /// message-content contribution declares `forwardable`. Plugin cards are
    /// bound to their conversation by default, so an unknown card, an
    /// unreadable catalog or a signed-out session all answer `false`.
    pub async fn plugin_card_forwardable(
        &self,
        message: &flare_plugin_protocol::ui::PluginMessage,
    ) -> bool {
        match self.fetch_plugin_ui_catalog().await {
            Ok((_, catalog)) => crate::extension::capability::ui::contribution_of(
                &catalog,
                message,
                Some(flare_plugin_protocol::manifest::UiContributionKind::MessageContent),
            )
            .is_some_and(|contribution| contribution.forwardable),
            Err(error) => {
                tracing::debug!(target = "flare_sdk.plugin", %error, "plugin catalog unavailable; forward hidden");
                false
            }
        }
    }
    /// Composer ("+" panel) entries of the plugins the current user may use on
    /// `platform`, titles in `locale`, sorted by order then title. Reads a fresh
    /// catalog each call; no further grant check, the server already trimmed it.
    pub async fn composer_actions(
        &self,
        platform: flare_plugin_protocol::manifest::ClientPlatform,
        locale: &str,
    ) -> Result<Vec<crate::extension::capability::ui::PluginComposerAction>> {
        self.session_guard
            .run(async {
                let (_, catalog) = self.fetch_plugin_ui_catalog().await?;
                Ok(crate::extension::capability::ui::composer_actions(
                    &self.default_tenant_id,
                    &catalog,
                    platform,
                    locale,
                ))
            })
            .await
    }
    /// Binding form of [`Self::composer_actions`]: `{platform, locale}` in,
    /// `{actions: [{pluginId, contributionId, title, icon?, order, audience?}]}`
    /// out. `audience` (`{conversationTypes?, groupRoles?}`) is for the host to
    /// filter by the open conversation and the viewer's role in it.
    pub async fn plugin_composer_actions(&self, request: Value) -> Result<Value> {
        let request = crate::extension::capability::ui::parse_composer_actions(request)?;
        let actions = self
            .composer_actions(request.platform, &request.locale)
            .await?;
        Ok(serde_json::json!({ "actions": actions }))
    }
    /// Refresh on login, tenant change and plugin state change. The returned
    /// catalog expires; callers must drop it on logout and never persist secrets.
    /// Each plugin carries `configuration`: the client-visible subset of its
    /// tenant configuration (keys marked `x-flare-client-visible`), absent when
    /// empty.
    pub async fn plugin_ui_catalog(&self) -> Result<Value> {
        Ok(self.fetch_plugin_ui_catalog().await?.0)
    }
    async fn fetch_plugin_ui_catalog(
        &self,
    ) -> Result<(Value, flare_plugin_protocol::ui::UiCatalog)> {
        let result = self
            .dispatch(
                flare_plugin_protocol::ui::UI_CATALOG_OPERATION,
                Value::Null,
                None,
                None,
                None,
            )
            .await?;
        if !result.success {
            return Err(FlareError::system("plugin UI catalog unavailable"));
        }
        let catalog: flare_plugin_protocol::ui::UiCatalog =
            serde_json::from_value(result.data.clone())
                .map_err(|_| FlareError::system("invalid plugin UI catalog"))?;
        if catalog.plugins.len() > 128 {
            return Err(FlareError::system("plugin UI catalog limit"));
        }
        Ok((result.data, catalog))
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
        if self.http.is_some() {
            return self
                .dispatch_http(capability_id, payload, conversation_id, tenant_id, user_id)
                .await;
        }
        let api = self.clone();
        let capability_id = capability_id.to_string();
        let conversation_id = conversation_id.map(str::to_string);
        let tenant_id = tenant_id.map(str::to_string);
        let user_id = user_id.map(str::to_string);
        self.session_guard
            .run_with_user(move |session_user_id| async move {
                let user = session_user_id
                    .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未连接"))?;
                if tenant_id
                    .as_deref()
                    .is_some_and(|id| id != api.default_tenant_id)
                    || user_id.as_deref().is_some_and(|id| id != user)
                {
                    return Err(FlareError::localized(
                        ErrorCode::InvalidParameter,
                        "sdk.capability.identity_override_denied",
                    ));
                }
                let tenant = api.default_tenant_id.clone();
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
                    request_id: uuid::Uuid::new_v4().to_string(),
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
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.dispatch_http(capability_id, payload, conversation_id, tenant_id, user_id)
            .await
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
