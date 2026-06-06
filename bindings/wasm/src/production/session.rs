//! Production wasm session — mirrors Tauri `SdkState`.

use std::future::Future;
use std::sync::Arc;

use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient, SdkConfigOverlay};
use flare_im_core_sdk_bindings_runtime::{InvokeSession, SessionSlot};

pub struct WasmSdkState {
    client: IMClient,
    session: SessionSlot,
}

impl WasmSdkState {
    pub fn new() -> Self {
        Self {
            client: IMClient::new(),
            session: SessionSlot::default(),
        }
    }

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

    pub async fn message_build_api(&self) -> Result<Arc<MessageBuildApi>> {
        self.session.message_build_api(&self.client).await
    }
}

impl InvokeSession for WasmSdkState {
    fn client(&self) -> IMClient {
        self.client.clone()
    }

    fn message_api(&self) -> impl Future<Output = Result<MessageApi>> + Send {
        async move { WasmSdkState::message_api(self).await }
    }

    fn message_build_api(&self) -> impl Future<Output = Result<Arc<MessageBuildApi>>> + Send {
        async move { WasmSdkState::message_build_api(self).await }
    }

    fn conversation_api(&self) -> impl Future<Output = Result<ConversationApi>> + Send {
        async move { WasmSdkState::conversation_api(self).await }
    }

    fn media_api(&self) -> impl Future<Output = Result<Arc<MediaApi>>> + Send {
        async move { WasmSdkState::media_api(self).await }
    }

    fn capability_api(&self) -> impl Future<Output = Result<Arc<CapabilityApi>>> + Send {
        async move { WasmSdkState::capability_api(self).await }
    }

    fn after_disconnect(&self) -> impl Future<Output = ()> + Send {
        async move {
            WasmSdkState::clear_session(self).await;
        }
    }
}

impl Default for WasmSdkState {
    fn default() -> Self {
        Self::new()
    }
}
