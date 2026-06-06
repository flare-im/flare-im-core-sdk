//! Tauri `State`：持有唯一 [`IMClient`] 与登录后的 [`ConnectedApis`] 快照。
//!
//! 热路径优先读 `session` 缓存；未安装或代际变化时回退 `IMClient::connected_apis()` 并自动刷新，
//! 避免 `sdk_login` 返回前同步事件触发 IPC 时报「未登录或会话未就绪」。

use std::future::Future;
use std::sync::Arc;

use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient, SdkConfigOverlay};
use flare_im_core_sdk::infrastructure::persistence::StoreProvider;
use flare_im_core_sdk_bindings_runtime::{InvokeSession, SessionSlot};

/// 由 `tauri::Builder::manage(SdkState::new())` 注入。
pub struct SdkState {
    client: IMClient,
    session: SessionSlot,
}

impl SdkState {
    pub fn new() -> Self {
        Self {
            client: IMClient::new(),
            session: SessionSlot::default(),
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
        self.session.install(&self.client, apis).await;
    }

    pub async fn clear_session(&self) {
        self.session.clear().await;
    }

    pub async fn logout(&self) -> Result<()> {
        self.clear_session().await;
        self.client.logout().await
    }

    pub async fn message_api(&self) -> Result<MessageApi> {
        self.session.message_api(&self.client).await
    }

    pub async fn conversation_api(&self) -> Result<ConversationApi> {
        self.session.conversation_api(&self.client).await
    }

    pub async fn media_api(&self) -> Result<Arc<MediaApi>> {
        self.session.media_api(&self.client).await
    }

    pub async fn capability_api(&self) -> Result<Arc<CapabilityApi>> {
        self.session.capability_api(&self.client).await
    }

    pub async fn presence_api(&self) -> Result<Arc<PresenceApi>> {
        self.session.presence_api(&self.client).await
    }

    pub async fn message_build_api(&self) -> Result<Arc<MessageBuildApi>> {
        self.session.message_build_api(&self.client).await
    }

    pub async fn stores(&self) -> Result<StoreProvider> {
        self.client.stores_async().await
    }
}

impl InvokeSession for SdkState {
    fn client(&self) -> IMClient {
        self.client.clone()
    }

    fn message_api(&self) -> impl Future<Output = Result<MessageApi>> + Send {
        async move { SdkState::message_api(self).await }
    }

    fn message_build_api(&self) -> impl Future<Output = Result<Arc<MessageBuildApi>>> + Send {
        async move { SdkState::message_build_api(self).await }
    }

    fn conversation_api(&self) -> impl Future<Output = Result<ConversationApi>> + Send {
        async move { SdkState::conversation_api(self).await }
    }

    fn media_api(&self) -> impl Future<Output = Result<Arc<MediaApi>>> + Send {
        async move { SdkState::media_api(self).await }
    }

    fn capability_api(&self) -> impl Future<Output = Result<Arc<CapabilityApi>>> + Send {
        async move { SdkState::capability_api(self).await }
    }

    fn after_disconnect(&self) -> impl Future<Output = ()> + Send {
        async move {
            SdkState::clear_session(self).await;
        }
    }
}

impl Default for SdkState {
    fn default() -> Self {
        Self::new()
    }
}
