//! IM [`ConnectedApis`] 快照：C FFI 热路径避免 `IMClient::try_read` 返回 lock busy。

use std::sync::Arc;

use flare_im_core_sdk::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi,
};
use flare_im_core_sdk::client::{ConnectedApis, IMClient};
use flare_im_core_sdk::Result;
use tokio::sync::RwLock;

#[derive(Clone)]
struct ImSessionCache {
    generation: u64,
    apis: ConnectedApis,
}

/// 每句柄一份，与 Tauri `SdkState::im_session` 策略一致。
#[derive(Clone, Default)]
pub struct ImSessionSlot {
    inner: Arc<RwLock<Option<ImSessionCache>>>,
}

impl ImSessionSlot {
    pub async fn install(&self, client: &IMClient, apis: ConnectedApis) {
        let generation = client.session_generation().await;
        *self.inner.write().await = Some(ImSessionCache {
            generation,
            apis,
        });
    }

    pub async fn clear(&self) {
        *self.inner.write().await = None;
    }

    async fn session_or_live(&self, client: &IMClient) -> Result<ConnectedApis> {
        let generation = client.session_generation().await;
        if let Some(cache) = self.inner.read().await.as_ref() {
            if cache.generation == generation {
                return Ok(cache.apis.clone());
            }
        }
        let apis = client.connected_apis().await?;
        *self.inner.write().await = Some(ImSessionCache {
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
