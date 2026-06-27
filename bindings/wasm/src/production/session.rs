//! Production wasm session — mirrors Tauri `SdkState`.

use std::sync::Arc;

use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient, SdkConfigOverlay};
use flare_im_core_sdk_bindings_runtime::{InvokeSession, SessionSlot, SessionTaskSlot};

pub struct WasmSdkState {
    client: IMClient,
    session: SessionSlot,
    event_bridge: SessionTaskSlot,
}

impl WasmSdkState {
    pub fn new() -> Self {
        Self {
            client: IMClient::new(),
            session: SessionSlot::default(),
            event_bridge: SessionTaskSlot::default(),
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

    pub fn event_bridge(&self) -> SessionTaskSlot {
        self.event_bridge.clone()
    }

    pub fn clear_event_bridge(&self) {
        self.event_bridge.clear();
    }

    pub async fn logout(&self) -> Result<()> {
        self.clear_event_bridge();
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

    async fn message_api(&self) -> Result<MessageApi> {
        WasmSdkState::message_api(self).await
    }

    async fn message_build_api(&self) -> Result<Arc<MessageBuildApi>> {
        WasmSdkState::message_build_api(self).await
    }

    async fn conversation_api(&self) -> Result<ConversationApi> {
        WasmSdkState::conversation_api(self).await
    }

    async fn media_api(&self) -> Result<Arc<MediaApi>> {
        WasmSdkState::media_api(self).await
    }

    async fn capability_api(&self) -> Result<Arc<CapabilityApi>> {
        WasmSdkState::capability_api(self).await
    }

    async fn after_disconnect(&self) {
        WasmSdkState::clear_session(self).await;
    }
}

impl Default for WasmSdkState {
    fn default() -> Self {
        Self::new()
    }
}
