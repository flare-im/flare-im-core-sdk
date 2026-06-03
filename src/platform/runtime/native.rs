//! Native runtime assembly.
//!
//! Native platforms keep using the existing `StoreProvider` and socket
//! transport. This module gives Android/iOS/Flutter/Tauri/Electron/RN native
//! bridges a stable assembly point without changing the public IM client APIs.

use async_trait::async_trait;

use crate::infrastructure::persistence::StoreProvider;
use crate::platform::adapters::storage::open_store_from_runtime_config;
use crate::platform::runtime::{RuntimeAssembler, RuntimeComponents, RuntimeConfig};
use crate::shared::error::Result;

pub struct NativeRuntimeAssembler {
    stores: Option<StoreProvider>,
}

impl NativeRuntimeAssembler {
    pub fn new(stores: StoreProvider) -> Self {
        Self {
            stores: Some(stores),
        }
    }

    pub fn configured() -> Self {
        Self { stores: None }
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
            None => open_native_stores_from_config(&config).await?,
        };
        Ok(RuntimeComponents::native_default(config.sdk, stores))
    }
}

async fn open_native_stores_from_config(config: &RuntimeConfig) -> Result<StoreProvider> {
    open_store_from_runtime_config(config).await
}
