//! SDK State Management
//!
//! Holds the singleton instance of the Flare IM Core SDK.
//! Thread-safe and accessible from all Tauri commands.

use std::sync::Arc;
use tokio::sync::RwLock;
use flare_im_core_sdk::interface::facade::ImCoreSdk;

/// Global SDK State wrapper for Tauri
///
/// This struct is managed by Tauri's state management system.
/// It uses RwLock to allow concurrent reads (queries) and exclusive writes (initialization/reset).
#[derive(Default)]
pub struct SdkState {
    /// The inner SDK instance
    inner: RwLock<Option<Arc<ImCoreSdk>>>,
}

impl SdkState {
    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Set the SDK instance (Initialization)
    pub async fn set_sdk(&self, sdk: Arc<ImCoreSdk>) {
        let mut lock = self.inner.write().await;
        *lock = Some(sdk);
    }

    /// Get a reference to the SDK instance
    ///
    /// Returns None if SDK is not initialized.
    pub async fn get_sdk(&self) -> Option<Arc<ImCoreSdk>> {
        let lock = self.inner.read().await;
        lock.clone()
    }

    /// Check if SDK is initialized
    pub async fn is_initialized(&self) -> bool {
        let lock = self.inner.read().await;
        lock.is_some()
    }

    /// Clear the SDK instance (Logout/Reset)
    pub async fn clear(&self) {
        let mut lock = self.inner.write().await;
        *lock = None;
    }
}
