//! [`SdkCapabilityRegistry`]：按 `capability_id` 前缀解析插件并派发；并可注册能力包观察者（与总线 `on_capability` 联动）。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use flare_proto::common::CapabilityPacket;
use serde_json::Value;

use crate::client::api::session_guard::SessionGuard;
use crate::client::api::{CapabilityDispatchResult, UserCapabilityGrantDto};
use crate::core::CurrentUserIdStore;
use crate::extension::capability::SdkCapabilityPlugin;
use crate::shared::error::{ErrorCode, FlareError, Result};

type CapabilityObserver = Arc<dyn Fn(&str, &CapabilityPacket) + Send + Sync>;

pub struct SdkCapabilityRegistry {
    session_guard: SessionGuard,
    plugins: RwLock<HashMap<String, Arc<dyn SdkCapabilityPlugin>>>,
    namespace_index: RwLock<HashMap<String, String>>,
    capability_observers: RwLock<Vec<CapabilityObserver>>,
}

impl SdkCapabilityRegistry {
    pub fn new() -> Self {
        Self {
            session_guard: SessionGuard::disabled("capability registry"),
            plugins: RwLock::new(HashMap::new()),
            namespace_index: RwLock::new(HashMap::new()),
            capability_observers: RwLock::new(Vec::new()),
        }
    }

    pub(crate) fn new_session_bound(current_user_id: CurrentUserIdStore) -> Self {
        Self {
            session_guard: SessionGuard::new(current_user_id, "capability registry"),
            plugins: RwLock::new(HashMap::new()),
            namespace_index: RwLock::new(HashMap::new()),
            capability_observers: RwLock::new(Vec::new()),
        }
    }

    /// 注册下行能力包观察者（在 [`crate::core::event::EventBus::on_capability`] 与构建器桥接之后收到与 UI 相同的能力包）。
    pub fn register_capability_observer<F>(&self, f: F)
    where
        F: Fn(&str, &CapabilityPacket) + Send + Sync + 'static,
    {
        if let Ok(mut g) = self.capability_observers.write() {
            g.push(Arc::new(f));
        }
    }

    /// 由构建器在 `MessageEvent::Capability` 发布路径上调用，转发给已注册观察者。
    pub fn dispatch_capability_to_observers(
        &self,
        conversation_id: &str,
        packet: &CapabilityPacket,
    ) {
        if let Ok(list) = self.capability_observers.read() {
            for f in list.iter() {
                f(conversation_id, packet);
            }
        }
    }

    pub fn register(&self, plugin: Arc<dyn SdkCapabilityPlugin>) -> Result<()> {
        let plugin_id = plugin.plugin_id().to_string();
        let mut plugins = self.plugins.write().map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "sdk.capability.registry_lock_failed",
            )
        })?;
        if plugins.contains_key(&plugin_id) {
            return Err(FlareError::localized(
                ErrorCode::OperationNotSupported,
                format!("sdk.capability.plugin_already_registered:{plugin_id}"),
            ));
        }
        let mut index = self.namespace_index.write().map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "sdk.capability.registry_lock_failed",
            )
        })?;
        for ns in plugin.capability_namespaces() {
            if index.contains_key(*ns) {
                return Err(FlareError::localized(
                    ErrorCode::OperationNotSupported,
                    format!("sdk.capability.namespace_already_registered:{ns}"),
                ));
            }
            index.insert((*ns).to_string(), plugin_id.clone());
        }
        plugins.insert(plugin_id, plugin);
        Ok(())
    }

    /// 注册插件并允许覆盖已有命名空间映射（用于私有发行注入商业插件）。
    ///
    /// 注意：该接口不会移除被覆盖插件实例，仅会替换命名空间路由。
    pub fn register_with_namespace_override(
        &self,
        plugin: Arc<dyn SdkCapabilityPlugin>,
    ) -> Result<()> {
        let plugin_id = plugin.plugin_id().to_string();
        let mut plugins = self.plugins.write().map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "sdk.capability.registry_lock_failed",
            )
        })?;
        let mut index = self.namespace_index.write().map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "sdk.capability.registry_lock_failed",
            )
        })?;

        for ns in plugin.capability_namespaces() {
            index.insert((*ns).to_string(), plugin_id.clone());
        }

        plugins.insert(plugin_id, plugin);
        Ok(())
    }

    pub fn unregister(&self, plugin_id: &str) -> Result<()> {
        let mut plugins = self.plugins.write().map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "sdk.capability.registry_lock_failed",
            )
        })?;
        if plugins.remove(plugin_id).is_none() {
            return Ok(());
        }
        let mut index = self.namespace_index.write().map_err(|_| {
            FlareError::localized(
                ErrorCode::InternalError,
                "sdk.capability.registry_lock_failed",
            )
        })?;
        index.retain(|_, v| v != plugin_id);
        Ok(())
    }

    pub fn get_plugin(&self, plugin_id: &str) -> Option<Arc<dyn SdkCapabilityPlugin>> {
        self.plugins
            .read()
            .ok()
            .and_then(|plugins| plugins.get(plugin_id).cloned())
    }

    pub fn resolve_by_capability(
        &self,
        capability_id: &str,
    ) -> Option<Arc<dyn SdkCapabilityPlugin>> {
        let namespace = capability_id.split('.').next().unwrap_or_default();
        let plugin_id = self
            .namespace_index
            .read()
            .ok()
            .and_then(|idx| idx.get(namespace).cloned())?;
        self.get_plugin(plugin_id.as_str())
    }

    /// 按 `capability_id` 首段命名空间解析插件并 `invoke`（推荐宿主调用扩展能力的主入口）。
    pub async fn invoke(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.session_guard.ensure_active().await?;
        let Some(plugin) = self.resolve_by_capability(capability_id) else {
            return Err(FlareError::localized(
                ErrorCode::OperationNotSupported,
                format!("sdk.capability.no_plugin_for:{capability_id}"),
            ));
        };
        plugin
            .invoke(capability_id, payload, conversation_id, tenant_id)
            .await
    }

    /// 解析处理该 `capability_id` 命名空间的插件并列出用户授权（与 [`SdkCapabilityPlugin::list_user_grants`] 一致）。
    pub async fn list_user_grants_for_capability(
        &self,
        capability_id: &str,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>> {
        self.session_guard.ensure_active().await?;
        let Some(plugin) = self.resolve_by_capability(capability_id) else {
            return Err(FlareError::localized(
                ErrorCode::OperationNotSupported,
                format!("sdk.capability.no_plugin_for:{capability_id}"),
            ));
        };
        plugin.list_user_grants(tenant_id, user_id).await
    }

    pub fn list_plugin_ids(&self) -> Vec<String> {
        self.plugins
            .read()
            .map(|plugins| plugins.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for SdkCapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::RwLock;

    struct CountingPlugin {
        invoke_calls: Arc<AtomicUsize>,
        grant_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl SdkCapabilityPlugin for CountingPlugin {
        fn plugin_id(&self) -> &'static str {
            "test.plugin"
        }

        fn capability_namespaces(&self) -> &'static [&'static str] {
            &["test"]
        }

        async fn invoke(
            &self,
            capability_id: &str,
            _payload: Value,
            _conversation_id: Option<&str>,
            _tenant_id: Option<&str>,
        ) -> Result<CapabilityDispatchResult> {
            self.invoke_calls.fetch_add(1, Ordering::SeqCst);
            Ok(CapabilityDispatchResult {
                request_id: String::new(),
                success: true,
                plugin_id: self.plugin_id().to_string(),
                capability_id: capability_id.to_string(),
                data: Value::Null,
                error: None,
            })
        }

        async fn list_user_grants(
            &self,
            _tenant_id: Option<&str>,
            _user_id: Option<&str>,
        ) -> Result<Vec<UserCapabilityGrantDto>> {
            self.grant_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn session_bound_registry_rejects_plugin_invocation_after_logout() {
        let current_user_id = Arc::new(RwLock::new("alice".to_string()));
        let registry = SdkCapabilityRegistry::new_session_bound(current_user_id.clone());
        let invoke_calls = Arc::new(AtomicUsize::new(0));
        let grant_calls = Arc::new(AtomicUsize::new(0));
        registry
            .register(Arc::new(CountingPlugin {
                invoke_calls: invoke_calls.clone(),
                grant_calls: grant_calls.clone(),
            }))
            .expect("register test plugin");

        *current_user_id.write().await = String::new();

        let err = registry
            .invoke("test.echo", Value::Null, None, None)
            .await
            .expect_err("logged-out registry invoke must fail");
        assert_eq!(err.code(), Some(ErrorCode::NotConnected));
        assert_eq!(invoke_calls.load(Ordering::SeqCst), 0);

        let err = registry
            .list_user_grants_for_capability("test.echo", None, None)
            .await
            .expect_err("logged-out registry grants must fail");
        assert_eq!(err.code(), Some(ErrorCode::NotConnected));
        assert_eq!(grant_calls.load(Ordering::SeqCst), 0);
    }
}
