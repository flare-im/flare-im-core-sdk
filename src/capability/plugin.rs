//! [`SdkCapabilityPlugin`]：扩展能力在 SDK 内的统一调用面（由独立 `flare-sdk-plugin-*` crate 约定 ID / payload）。

use async_trait::async_trait;
use serde_json::Value;

use crate::client::api::{CapabilityDispatchResult, UserCapabilityGrantDto};
use crate::error::Result;

#[async_trait]
pub trait SdkCapabilityPlugin: Send + Sync {
    fn plugin_id(&self) -> &'static str;
    fn capability_namespaces(&self) -> &'static [&'static str];

    async fn invoke(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult>;

    async fn list_user_grants(
        &self,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>>;
}
