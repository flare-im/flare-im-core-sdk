//! Binding-neutral UI evaluation. Native adapters render the result through the
//! design kit and execute only a returned target after explicit host consent.
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_plugin_protocol::{
    manifest::{ClientPlatform, UiContributionKind, is_ui_icon_name},
    ui::{NativeCapability, PluginMessage, UiCatalog},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResolveRequest {
    catalog: UiCatalog,
    message: PluginMessage,
    platform: ClientPlatform,
    #[serde(default)]
    locale: String,
    /// Signed-in user, set by the SDK session; never taken from plugin payload.
    #[serde(default)]
    viewer: String,
    #[serde(default)]
    kind: Option<UiContributionKind>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActionRequest {
    catalog: UiCatalog,
    platform: ClientPlatform,
    plugin_id: String,
    contribution_id: String,
    action_id: String,
    native_allowlist: BTreeSet<NativeCapability>,
    user_gesture: bool,
    payload: Value,
}
fn invalid() -> FlareError {
    FlareError::localized(ErrorCode::InvalidParameter, "sdk.plugin.invalid_ui_request")
}
fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
fn bounded(value: &Value) -> Result<()> {
    if serde_json::to_vec(value).map_err(|_| invalid())?.len() > 2 * 1024 * 1024 {
        return Err(invalid());
    }
    Ok(())
}
/// Network orchestration inputs never accept a caller-provided catalog, tenant
/// or actor. Every click refreshes the authenticated catalog in CapabilityApi.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PresentRequest {
    pub message: PluginMessage,
    pub platform: ClientPlatform,
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub kind: Option<UiContributionKind>,
    /// Message the card belongs to (server id or client message id). Its stored
    /// plugin state is overlaid on the payload before resolving.
    #[serde(default)]
    pub message_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InvokeRequest {
    pub message: PluginMessage,
    pub platform: ClientPlatform,
    pub action_id: String,
    pub user_gesture: bool,
    #[serde(default)]
    pub native_allowlist: BTreeSet<NativeCapability>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Message the action came from. Only then is a returned `data.messageState`
    /// written back to that message.
    #[serde(default)]
    pub message_id: Option<String>,
}
fn valid_message_id(id: &Option<String>) -> bool {
    id.as_deref().is_none_or(|id| {
        !id.is_empty()
            && id.len() <= 256
            && !id.chars().any(|c| c.is_control() || c.is_whitespace())
    })
}
pub(crate) fn parse_present(request: Value) -> Result<PresentRequest> {
    bounded(&request)?;
    let parsed: PresentRequest = serde_json::from_value(request).map_err(|_| invalid())?;
    parsed.message.validate().map_err(|_| invalid())?;
    if !valid_message_id(&parsed.message_id) {
        return Err(invalid());
    }
    Ok(parsed)
}
pub(crate) fn parse_invoke(request: Value) -> Result<InvokeRequest> {
    bounded(&request)?;
    let parsed: InvokeRequest = serde_json::from_value(request).map_err(|_| invalid())?;
    parsed.message.validate().map_err(|_| invalid())?;
    if parsed
        .conversation_id
        .as_ref()
        .is_some_and(|id| !flare_plugin_protocol::resource::valid_resource_id(id))
        || !valid_message_id(&parsed.message_id)
    {
        return Err(invalid());
    }
    Ok(parsed)
}
/// A "+" panel entry declared by an authorized plugin (`composer-action`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginComposerAction {
    pub plugin_id: String,
    pub contribution_id: String,
    /// Contribution title in the requested locale (exact tag, then language).
    pub title: String,
    /// Kit icon name; absent when undeclared or malformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub order: i32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComposerActionsRequest {
    pub platform: ClientPlatform,
    #[serde(default)]
    pub locale: String,
}
pub(crate) fn parse_composer_actions(request: Value) -> Result<ComposerActionsRequest> {
    bounded(&request)?;
    let parsed: ComposerActionsRequest = serde_json::from_value(request).map_err(|_| invalid())?;
    if parsed.locale.len() > 32 || parsed.locale.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(parsed)
}
/// Composer actions of `catalog` for `platform`, by order then title. The
/// server already trimmed the catalog by grant, capability policy and user
/// consent, so this only drops what a client cannot render: another tenant's
/// or an expired catalog, other platforms, unknown schema versions and invalid
/// documents.
pub(crate) fn composer_actions(
    tenant: &str,
    catalog: &UiCatalog,
    platform: ClientPlatform,
    locale: &str,
) -> Vec<PluginComposerAction> {
    if catalog.tenant_id != tenant || catalog.valid_until_ms <= now_ms() {
        return Vec::new();
    }
    let mut actions: Vec<_> = catalog
        .plugins
        .iter()
        .filter(|plugin| plugin.extensions.platforms.contains(&platform))
        .flat_map(|plugin| {
            plugin
                .extensions
                .contributions
                .iter()
                .filter(|c| {
                    c.kind == UiContributionKind::ComposerAction
                        && c.schema_version == 1
                        && c.schema.validate().is_ok()
                })
                .map(|c| PluginComposerAction {
                    plugin_id: plugin.plugin_id.clone(),
                    contribution_id: c.id.clone(),
                    title: c.schema.localized_title(locale).to_owned(),
                    icon: c.icon.clone().filter(|icon| is_ui_icon_name(icon)),
                    order: c.order,
                })
        })
        .collect();
    actions.sort_by(|a, b| {
        (a.order, &a.title, &a.contribution_id).cmp(&(b.order, &b.title, &b.contribution_id))
    });
    actions
}
pub(crate) fn resolve(tenant: &str, request: Value) -> Result<Value> {
    bounded(&request)?;
    let request: ResolveRequest = serde_json::from_value(request).map_err(|_| invalid())?;
    serde_json::to_value(request.catalog.resolve_with_context(
        &request.message,
        flare_plugin_protocol::ui::UiRenderContext {
            tenant,
            platform: request.platform,
            kind: request.kind.unwrap_or(UiContributionKind::MessageContent),
            locale: &request.locale,
            viewer: &request.viewer,
            now_ms: now_ms(),
        },
    ))
    .map_err(|_| invalid())
}
pub(crate) fn action(tenant: &str, request: Value) -> Result<Value> {
    bounded(&request)?;
    let request: ActionRequest = serde_json::from_value(request).map_err(|_| invalid())?;
    let target = request
        .catalog
        .action(
            tenant,
            &request.plugin_id,
            &request.contribution_id,
            &request.action_id,
            request.platform,
            &request.native_allowlist,
            request.user_gesture,
            now_ms(),
        )
        .map_err(|_| {
            FlareError::localized(
                ErrorCode::OperationNotSupported,
                "sdk.plugin.ui_action_unavailable",
            )
        })?;
    let action = request
        .catalog
        .plugins
        .iter()
        .find(|p| p.plugin_id == request.plugin_id)
        .and_then(|p| {
            p.extensions
                .contributions
                .iter()
                .find(|c| c.id == request.contribution_id)
        })
        .and_then(|c| c.schema.actions.iter().find(|a| a.id == request.action_id))
        .ok_or_else(invalid)?;
    let payload = flare_plugin_protocol::ui::bind_action_input(action, &request.payload)
        .map_err(|_| invalid())?;
    Ok(serde_json::json!({"target":target,"payload":payload}))
}
