//! Shared connected API session cache for platform bindings.
//!
//! C, Tauri, and wasm all need the same generation-aware `ConnectedApis`
//! cache. Keeping it here prevents each binding from inventing subtly
//! different hot-path behavior.

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use flare_im_core_sdk::Result;
use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi, ViewApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient};

#[derive(Clone)]
struct SessionCache {
    generation: u64,
    apis: ConnectedApis,
}

/// Per-binding-handle cache for the API facades of the active IM session.
#[derive(Clone, Default)]
pub struct SessionSlot {
    inner: Arc<RwLock<Option<SessionCache>>>,
}

impl SessionSlot {
    fn read_cache(&self) -> RwLockReadGuard<'_, Option<SessionCache>> {
        match self.inner.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn write_cache(&self) -> RwLockWriteGuard<'_, Option<SessionCache>> {
        match self.inner.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Install a freshly returned API snapshot after login.
    pub async fn install(&self, client: &IMClient, apis: ConnectedApis) {
        let generation = client.session_generation_snapshot();
        *self.write_cache() = Some(SessionCache { generation, apis });
    }

    /// Clear cached API facades after logout/disconnect/uninit.
    pub async fn clear(&self) {
        *self.write_cache() = None;
    }

    async fn session_or_live(&self, client: &IMClient) -> Result<ConnectedApis> {
        let generation = client.session_generation_snapshot();
        if let Some(apis) = self
            .read_cache()
            .as_ref()
            .filter(|cache| cache.generation == generation)
            .map(|cache| cache.apis.clone())
        {
            return Ok(apis);
        }

        let apis = client.connected_apis().await?;
        let generation = client.session_generation_snapshot();
        *self.write_cache() = Some(SessionCache {
            generation,
            apis: apis.clone(),
        });
        Ok(apis)
    }

    pub async fn message_api(&self, client: &IMClient) -> Result<MessageApi> {
        Ok(self.session_or_live(client).await?.message_api)
    }

    pub async fn message_build_api(&self, client: &IMClient) -> Result<Arc<MessageBuildApi>> {
        Ok(self.session_or_live(client).await?.message_build_api)
    }

    pub async fn conversation_api(&self, client: &IMClient) -> Result<ConversationApi> {
        Ok(self.session_or_live(client).await?.conversation_api)
    }

    pub async fn view_api(&self, client: &IMClient) -> Result<Arc<ViewApi>> {
        Ok(self.session_or_live(client).await?.view_api)
    }

    pub async fn media_api(&self, client: &IMClient) -> Result<Arc<MediaApi>> {
        Ok(self.session_or_live(client).await?.media_api)
    }

    pub async fn capability_api(&self, client: &IMClient) -> Result<Arc<CapabilityApi>> {
        Ok(self.session_or_live(client).await?.capability_api)
    }

    pub async fn presence_api(&self, client: &IMClient) -> Result<Arc<PresenceApi>> {
        Ok(self.session_or_live(client).await?.presence_api)
    }
}
