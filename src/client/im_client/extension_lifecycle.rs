use std::sync::Arc;

use crate::extension::{ExtensionContext, ExtensionLifecycleContext, ExtensionRuntime};
use crate::shared::error::Result;

use super::IMClient;

impl IMClient {
    pub(super) async fn extension_lifecycle_snapshot(
        &self,
        current_user_id: Option<String>,
    ) -> Option<(Arc<ExtensionRuntime>, ExtensionLifecycleContext)> {
        let g = self.inner.read().await;
        let runtime = g.extension_runtime.clone()?;
        if runtime.lifecycle_count() == 0 {
            return None;
        }
        let engine = g.engine.as_ref()?;
        let current_user_id = current_user_id.or_else(|| g.current_user_id.clone());
        Some((
            runtime,
            ExtensionLifecycleContext::new(
                ExtensionContext::from_core(engine.stores().clone(), engine.bus().clone()),
                current_user_id,
            ),
        ))
    }

    pub(super) async fn notify_extension_login_best_effort(&self, user_id: &str) {
        if let Some((runtime, context)) = self
            .extension_lifecycle_snapshot(Some(user_id.to_string()))
            .await
            && let Err(err) = runtime.notify_login(&context).await
        {
            tracing::warn!(
                target = "flare_sdk.extension",
                user_id = user_id,
                error = %err,
                "SDK extension login lifecycle failed"
            );
        }
    }

    pub(super) async fn notify_extension_connect_best_effort(&self, user_id: &str) {
        if let Some((runtime, context)) = self
            .extension_lifecycle_snapshot(Some(user_id.to_string()))
            .await
            && let Err(err) = runtime.notify_connect(&context).await
        {
            tracing::warn!(
                target = "flare_sdk.extension",
                user_id = user_id,
                error = %err,
                "SDK extension connect lifecycle failed"
            );
        }
    }

    pub(super) async fn notify_extension_disconnect_snapshot(
        lifecycle: &Option<(Arc<ExtensionRuntime>, ExtensionLifecycleContext)>,
    ) {
        if let Some((runtime, context)) = lifecycle
            && let Err(err) = runtime.notify_disconnect(context).await
        {
            tracing::warn!(
                target = "flare_sdk.extension",
                user_id = context.current_user_id().unwrap_or_default(),
                error = %err,
                "SDK extension disconnect lifecycle failed"
            );
        }
    }

    pub(super) async fn notify_extension_logout_snapshot(
        lifecycle: &Option<(Arc<ExtensionRuntime>, ExtensionLifecycleContext)>,
    ) {
        if let Some((runtime, context)) = lifecycle
            && let Err(err) = runtime.notify_logout(context).await
        {
            tracing::warn!(
                target = "flare_sdk.extension",
                user_id = context.current_user_id().unwrap_or_default(),
                error = %err,
                "SDK extension logout lifecycle failed"
            );
        }
    }

    /// 获取扩展运行时快照。
    pub fn extension_runtime(&self) -> Result<Arc<ExtensionRuntime>> {
        self.read_inner()?
            .extension_runtime
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 构造受控扩展上下文，供业务 SDK 投影资料、会话等核心模型。
    pub async fn extension_context(&self) -> Result<ExtensionContext> {
        self.with_engine_async(|engine| {
            ExtensionContext::from_core(engine.stores().clone(), engine.bus().clone())
        })
        .await
    }
}
