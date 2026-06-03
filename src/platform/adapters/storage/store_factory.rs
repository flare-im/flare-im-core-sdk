//! Storage adapter factory.

use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::persistence::in_memory_empty_im_provider;
use crate::platform::runtime::{RuntimeConfig, StorageKind};
use crate::shared::error::{ErrorCode, FlareError, Result};

pub async fn open_store_from_runtime_config(config: &RuntimeConfig) -> Result<StoreProvider> {
    match config.storage.kind {
        StorageKind::Memory => Ok(in_memory_empty_im_provider()),
        StorageKind::Sqlite => open_sqlite_stores(config).await,
        StorageKind::IndexedDb => Err(FlareError::localized(
            ErrorCode::OperationNotSupported,
            "IndexedDB storage must be injected by the Web storage adapter",
        )),
        StorageKind::Custom => Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            "custom storage requires an injected StoreProvider",
        )),
    }
}

#[cfg(feature = "lifecycle-sqlite")]
async fn open_sqlite_stores(config: &RuntimeConfig) -> Result<StoreProvider> {
    use std::path::PathBuf;

    use crate::shared::util::{default_sdk_data_root, open_sqlite_store_for_user};

    let user_id = config.storage.user_id.trim();
    if user_id.is_empty() {
        return Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            "SQLite storage requires storage.user_id",
        ));
    }
    let base_data_dir = config
        .storage
        .path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(default_sdk_data_root);
    open_sqlite_store_for_user(&base_data_dir, user_id).await
}

#[cfg(not(feature = "lifecycle-sqlite"))]
async fn open_sqlite_stores(_config: &RuntimeConfig) -> Result<StoreProvider> {
    Err(FlareError::localized(
        ErrorCode::ConfigurationError,
        "SQLite storage requires the lifecycle-sqlite feature",
    ))
}
