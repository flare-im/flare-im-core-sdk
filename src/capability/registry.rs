//! [`SdkCapabilityRegistry`]：按 `capability_id` 前缀解析插件并派发；并可注册 **通话信令观察者**（与总线 `on_call_signal` 联动）。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use flare_proto::common::CallSignalEvent;
use serde_json::Value;

use crate::capability::SdkCapabilityPlugin;
use crate::client::api::{CapabilityDispatchResult, UserCapabilityGrantDto};
use crate::error::{ErrorCode, FlareError, Result};

type CallSignalObserver = Arc<dyn Fn(&str, &CallSignalEvent) + Send + Sync>;

pub struct SdkCapabilityRegistry {
    plugins: RwLock<HashMap<String, Arc<dyn SdkCapabilityPlugin>>>,
    namespace_index: RwLock<HashMap<String, String>>,
    call_signal_observers: RwLock<Vec<CallSignalObserver>>,
}

impl SdkCapabilityRegistry {
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            namespace_index: RwLock::new(HashMap::new()),
            call_signal_observers: RwLock::new(Vec::new()),
        }
    }

    /// 注册下行通话信令观察者（在 [`crate::event::EventBus::on_call_signal`] 与构建器桥接之后收到与 UI 相同的信令）。
    pub fn register_call_signal_observer<F>(&self, f: F)
    where
        F: Fn(&str, &CallSignalEvent) + Send + Sync + 'static,
    {
        if let Ok(mut g) = self.call_signal_observers.write() {
            g.push(Arc::new(f));
        }
    }

    /// 由构建器在 `MessageEvent::CallSignal` 发布路径上调用，转发给已注册观察者。
    pub fn dispatch_call_signal_to_observers(
        &self,
        conversation_id: &str,
        event: &CallSignalEvent,
    ) {
        if let Ok(list) = self.call_signal_observers.read() {
            for f in list.iter() {
                f(conversation_id, event);
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
