//! Plugin pages: `CapabilityApi::plugin_page` and page actions.
use serde_json::Value;

use super::{CapabilityApi, CapabilityDispatchResult};
use crate::extension::capability::ui_page;
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_plugin_protocol::{
    manifest::UiContributionKind,
    ui::{ResolvedPluginView, UiRenderContext, UiUnavailableReason},
};

impl CapabilityApi {
    /// A plugin page (a `page` contribution, such as a wallet), resolved like
    /// a card's detail: `{pluginId, page, platform, locale?, arguments?,
    /// filters?, cursor?, previous?, refresh?}` in, a v2 view out (`available`,
    /// `title`, `fields`, `actions`, `menuActions?`, `blocks?`, `inputs?`,
    /// `nextCursor?`, `unavailableReason?`) plus `data`, what it was resolved
    /// against, which a page action passes back.
    ///
    /// - `arguments`: the payload of the page target that opened the page
    ///   (`{kind: "host", target: {kind: "page"}, payload}` of an action);
    ///   `{}` from an account entry. The page's source (a declared query) is
    ///   called with its input bound from them; a page without a source is
    ///   resolved against them.
    /// - `filters`: values of the rows block's filter inputs, validated like a
    ///   form and joined to the source input.
    /// - Paging and refresh follow `plugin_detail`: `cursor` (the view's
    ///   `nextCursor`) with `previous` (the view as returned) appends the next
    ///   page to it; `refresh` with `previous` re-reads from the first page
    ///   until at least as many entries and rows as `previous` showed are
    ///   back, the source has no next page, or 10 pages were read.
    /// - The first page and a refresh never fail for an unavailable plugin:
    ///   an unreadable catalog, a disabled, uninstalled or not permitted
    ///   plugin, another platform or an unknown page give an inert view
    ///   (`available: false`, empty title, `unavailableReason`). A next page
    ///   fails instead, and any other failure of the source is an error.
    ///
    /// A page is no card: its calls carry no `messageId` and bind no entity.
    pub async fn plugin_page(&self, request: Value) -> Result<Value> {
        let mut request = ui_page::parse_page(request)?;
        let previous = request
            .previous
            .take()
            .map(ui_page::split_previous)
            .transpose()?;
        self.session_guard
            .run_with_user(|viewer| async move {
                let viewer = viewer.unwrap_or_default();
                let inert = |reason| {
                    ui_page::page_view(&ResolvedPluginView::unavailable_page(reason), None)
                };
                let catalog = match self.fetch_plugin_ui_catalog().await {
                    Ok((_, catalog)) => catalog,
                    Err(error) if error.code() == Some(ErrorCode::NotConnected) => {
                        return Err(error);
                    }
                    Err(_) if request.degrades() => {
                        return inert(UiUnavailableReason::CatalogUnreachable);
                    }
                    Err(error) => return Err(error),
                };
                let context = UiRenderContext {
                    tenant: &self.default_tenant_id,
                    platform: request.platform,
                    kind: UiContributionKind::Page,
                    locale: &request.locale,
                    viewer: &viewer,
                    now_ms: ui_page::now_ms(),
                };
                let contribution = match catalog.page(&request.plugin_id, &request.page, &context) {
                    Ok((_, contribution)) => contribution,
                    Err(reason) if request.degrades() => return inert(reason),
                    Err(_) => {
                        return Err(FlareError::localized(
                            ErrorCode::OperationNotSupported,
                            "sdk.plugin.ui_action_unavailable",
                        ));
                    }
                };
                let prepared = ui_page::prepare_page(contribution, &request)?;
                // A malformed cursor is refused before anything is sent.
                let input = prepared.input(request.cursor.as_deref())?;
                let resolve = |data: &Value| {
                    contribution
                        .resolve_page(data, &request.locale, &viewer)
                        .ok_or_else(|| {
                            FlareError::localized(
                                ErrorCode::OperationNotSupported,
                                "sdk.plugin.ui_action_unavailable",
                            )
                        })
                };
                let first = match self.page_data(&request, &prepared, input, &viewer).await {
                    Ok(data) => data,
                    Err(error) if request.degrades() => match ui_page::inert_reason(&error) {
                        Some(reason) => return inert(reason),
                        None => return Err(error),
                    },
                    Err(error) => return Err(error),
                };
                let mut view = resolve(&first)?;
                let mut data = first;
                match previous {
                    Some((previous, _)) if request.refresh => {
                        let wanted = ui_page::page_items(&previous);
                        let mut pages = 1;
                        while let Some(cursor) =
                            ui_page::refresh_continues(&view, wanted, pages).map(str::to_owned)
                        {
                            let input = prepared.input(Some(&cursor))?;
                            let next = self.page_data(&request, &prepared, input, &viewer).await?;
                            view.append_page(resolve(&next)?);
                            pages += 1;
                        }
                    }
                    Some((mut previous, previous_data)) => {
                        previous.append_page(view);
                        view = previous;
                        data = previous_data;
                    }
                    None => {}
                }
                ui_page::page_view(&view, Some(data))
            })
            .await
    }

    /// One page of the source result (the arguments of a page without a
    /// source). A failure that means the plugin is no longer available
    /// invalidates the catalog like any plugin call.
    async fn page_data(
        &self,
        request: &ui_page::PageRequest,
        prepared: &ui_page::PreparedPage,
        input: Value,
        viewer: &str,
    ) -> Result<Value> {
        let Some(source) = &prepared.source else {
            return Ok(input);
        };
        let viewer = (!viewer.is_empty()).then_some(viewer);
        match self
            .dispatch_with(
                &source.operation,
                input,
                None,
                Some(&self.default_tenant_id),
                viewer,
                None,
            )
            .await
            .and_then(CapabilityDispatchResult::fail_if_unsuccessful)
        {
            Ok(response) => Ok(response.data),
            Err(error) => {
                self.after_plugin_error(&error, &request.plugin_id).await;
                Err(error)
            }
        }
    }

    /// A page action (`plugin_ui_invoke` with `page` instead of `message`):
    /// checked and bound by [`ui_page::prepare_page_action`], then an
    /// operation target is dispatched without `messageId` (Core's card binding
    /// guard concerns card calls only); page and native targets come back to
    /// the host as `{kind: "host", target, payload}` (a page target's payload
    /// is the next page's `arguments`).
    pub(super) async fn invoke_page_action(&self, request: Value) -> Result<Value> {
        use crate::extension::capability::ui::PreparedAction;
        use flare_plugin_protocol::ui::UiActionTarget;
        let request = ui_page::parse_page_action(request)?;
        self.session_guard
            .run_with_user(|user_id| async move {
                let (_, catalog) = self.fetch_plugin_ui_catalog().await?;
                match ui_page::prepare_page_action(&self.default_tenant_id, &catalog, &request)? {
                    PreparedAction::FormErrors(errors) => {
                        Ok(serde_json::json!({"kind":"formErrors","errors":errors}))
                    }
                    PreparedAction::Ready {
                        target: UiActionTarget::Operation { operation },
                        payload,
                        ..
                    } => {
                        let response = match self
                            .dispatch_with(
                                &operation,
                                payload,
                                request.conversation_id.as_deref(),
                                Some(&self.default_tenant_id),
                                user_id.as_deref(),
                                None,
                            )
                            .await
                        {
                            Ok(response) => response,
                            Err(error) => {
                                self.after_plugin_error(&error, &request.page.plugin_id)
                                    .await;
                                return Err(error);
                            }
                        };
                        Ok(serde_json::json!({"kind":"operation","response":response}))
                    }
                    PreparedAction::Ready {
                        target, payload, ..
                    } => Ok(serde_json::json!({"kind":"host","target":target,"payload":payload})),
                }
            })
            .await
    }
}
