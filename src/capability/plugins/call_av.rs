//! 音视频（RTC/SFU）能力插件：将 `rtc.*` capability 请求转发到 `CapabilityApi::dispatch`。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::capability::SdkCapabilityPlugin;
use crate::client::api::{CapabilityApi, CapabilityDispatchResult, UserCapabilityGrantDto};
use crate::error::Result;

const CALL_AV_PLUGIN_ID: &str = "sdk.plugin.av";
const RTC_CAPABILITY_NAMESPACE: &str = "rtc";

/// AV 插件（RTC/SFU），通过 capability 服务统一下发命令。
pub struct AvCapabilityPlugin {
    api: Arc<CapabilityApi>,
}

impl AvCapabilityPlugin {
    pub fn new(api: Arc<CapabilityApi>) -> Self {
        Self { api }
    }
}

#[async_trait]
impl SdkCapabilityPlugin for AvCapabilityPlugin {
    fn plugin_id(&self) -> &'static str {
        CALL_AV_PLUGIN_ID
    }

    fn capability_namespaces(&self) -> &'static [&'static str] {
        &[RTC_CAPABILITY_NAMESPACE]
    }

    async fn invoke(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.api
            .dispatch(capability_id, payload, conversation_id, tenant_id, None)
            .await
    }

    async fn list_user_grants(
        &self,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>> {
        self.api.list_user_capabilities(tenant_id, user_id).await
    }
}
