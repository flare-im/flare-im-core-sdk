//! Native runtime assembly.
//!
//! Native platforms keep using the existing `StoreProvider` and socket
//! transport. This module gives Android/iOS/Flutter/Tauri/Electron/RN native
//! bridges a stable assembly point without changing the public IM client APIs.

use async_trait::async_trait;
use std::sync::Arc;

use crate::infrastructure::persistence::StoreProvider;
use crate::platform::adapters::storage::open_store_from_runtime_config_with_secure_key_store;
use crate::platform::ports::storage::SecureKeyStore;
use crate::platform::runtime::{RuntimeAssembler, RuntimeComponents, RuntimeConfig};
use crate::shared::error::Result;

pub struct NativeRuntimeAssembler {
    stores: Option<StoreProvider>,
    secure_key_store: Option<Arc<dyn SecureKeyStore>>,
}

impl NativeRuntimeAssembler {
    pub fn new(stores: StoreProvider) -> Self {
        Self {
            stores: Some(stores),
            secure_key_store: None,
        }
    }

    pub fn configured() -> Self {
        Self {
            stores: None,
            secure_key_store: None,
        }
    }

    pub fn configured_with_secure_key_store(key_store: Arc<dyn SecureKeyStore>) -> Self {
        Self {
            stores: None,
            secure_key_store: Some(key_store),
        }
    }

    pub fn secure_key_store(&self) -> Option<&Arc<dyn SecureKeyStore>> {
        self.secure_key_store.as_ref()
    }

    pub fn stores(&self) -> Option<&StoreProvider> {
        self.stores.as_ref()
    }
}

#[async_trait]
impl RuntimeAssembler for NativeRuntimeAssembler {
    async fn assemble(&self, config: RuntimeConfig) -> Result<RuntimeComponents> {
        let stores = match &self.stores {
            Some(stores) => stores.clone(),
            None => open_native_stores_from_config(&config, self.secure_key_store.clone()).await?,
        };
        Ok(RuntimeComponents::native_default(config.sdk, stores))
    }
}

async fn open_native_stores_from_config(
    config: &RuntimeConfig,
    secure_key_store: Option<Arc<dyn SecureKeyStore>>,
) -> Result<StoreProvider> {
    open_store_from_runtime_config_with_secure_key_store(config, secure_key_store).await
}
