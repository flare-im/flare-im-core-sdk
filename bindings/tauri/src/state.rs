//! Tauri `State`：持有唯一 [`IMClient`] 与登录后的 [`ConnectedApis`] 快照。
//!
//! 热路径命令读取 `session`（`tokio::sync::RwLock` 读锁），避免与登录/同步争抢 `IMClient` 内部写锁。

use std::sync::Arc;

use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient, SdkConfigOverlay};
use flare_im_core_sdk::store::StoreProvider;
use flare_im_core_sdk::{ErrorCode, FlareError, Result};
use tokio::sync::RwLock;

/// 由 `tauri::Builder::manage(SdkState::new())` 注入。
pub struct SdkState {
    client: IMClient,
    session: RwLock<Option<ConnectedApis>>,
}

impl SdkState {
    pub fn new() -> Self {
        Self {
            client: IMClient::new(),
            session: RwLock::new(None),
        }
    }

    /// O(1)：`IMClient` 为 `Arc` 浅拷贝。
    #[inline]
    pub fn client(&self) -> IMClient {
        self.client.clone()
    }

    pub async fn set_config(
        &self,
        environment: Option<String>,
        sdk_config: Option<SdkConfigOverlay>,
    ) -> Result<()> {
        self.client.init(environment, sdk_config).await
    }

    pub async fn config_snapshot(&self) -> (Option<String>, Option<SdkConfigOverlay>) {
        self.client.config_snapshot().await
    }

    pub async fn install_session(&self, apis: ConnectedApis) {
        *self.session.write().await = Some(apis);
    }

    pub async fn clear_session(&self) {
        *self.session.write().await = None;
    }

    pub async fn logout(&self) -> Result<()> {
        self.clear_session().await;
        self.client.logout().await
    }

    async fn require_session(&self) -> Result<ConnectedApis> {
        self.session
            .read()
            .await
            .clone()
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未登录或会话未就绪"))
    }

    pub async fn message_api(&self) -> Result<MessageApi> {
        Ok(self.require_session().await?.message_api)
    }

    pub async fn conversation_api(&self) -> Result<ConversationApi> {
        Ok(self.require_session().await?.conversation_api)
    }

    pub async fn media_api(&self) -> Result<Arc<MediaApi>> {
        Ok(self.require_session().await?.media_api)
    }

    pub async fn capability_api(&self) -> Result<Arc<CapabilityApi>> {
        Ok(self.require_session().await?.capability_api)
    }

    pub async fn presence_api(&self) -> Result<Arc<PresenceApi>> {
        Ok(self.require_session().await?.presence_api)
    }

    pub async fn message_build_api(&self) -> Result<Arc<MessageBuildApi>> {
        Ok(self.require_session().await?.message_build_api)
    }

    pub async fn stores(&self) -> Result<StoreProvider> {
        self.client.stores_async().await
    }
}

impl Default for SdkState {
    fn default() -> Self {
        Self::new()
    }
}
