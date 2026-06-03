//! Tauri `State`：持有唯一 [`IMClient`] 与登录后的 [`ConnectedApis`] 快照。
//!
//! 热路径优先读 `session` 缓存；未安装或代际变化时回退 `IMClient::connected_apis()` 并自动刷新，
//! 避免 `sdk_login` 返回前同步事件触发 IPC 时报「未登录或会话未就绪」。

use std::sync::Arc;

use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient, SdkConfigOverlay};
use flare_im_core_sdk::infrastructure::persistence::StoreProvider;
use tokio::sync::RwLock;

#[derive(Clone)]
struct SessionCache {
    generation: u64,
    apis: ConnectedApis,
}

/// 由 `tauri::Builder::manage(SdkState::new())` 注入。
pub struct SdkState {
    client: IMClient,
    session: RwLock<Option<SessionCache>>,
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
        let generation = self.client.session_generation().await;
        *self.session.write().await = Some(SessionCache { generation, apis });
    }

    pub async fn clear_session(&self) {
        *self.session.write().await = None;
    }

    pub async fn logout(&self) -> Result<()> {
        self.clear_session().await;
        self.client.logout().await
    }

    /// 已缓存且代际一致则直接返回；否则从 `IMClient` 拉取并写回缓存。
    async fn session_or_live(&self) -> Result<ConnectedApis> {
        let generation = self.client.session_generation().await;
        if let Some(cache) = self.session.read().await.as_ref()
            && cache.generation == generation
        {
            return Ok(cache.apis.clone());
        }
        let apis = self.client.connected_apis().await?;
        *self.session.write().await = Some(SessionCache {
            generation,
            apis: apis.clone(),
        });
        Ok(apis)
    }

    pub async fn message_api(&self) -> Result<MessageApi> {
        Ok(self.session_or_live().await?.message_api)
    }

    pub async fn conversation_api(&self) -> Result<ConversationApi> {
        Ok(self.session_or_live().await?.conversation_api)
    }

    pub async fn media_api(&self) -> Result<Arc<MediaApi>> {
        Ok(self.session_or_live().await?.media_api)
    }

    pub async fn capability_api(&self) -> Result<Arc<CapabilityApi>> {
        Ok(self.session_or_live().await?.capability_api)
    }

    pub async fn presence_api(&self) -> Result<Arc<PresenceApi>> {
        Ok(self.session_or_live().await?.presence_api)
    }

    pub async fn message_build_api(&self) -> Result<Arc<MessageBuildApi>> {
        Ok(self.session_or_live().await?.message_build_api)
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
