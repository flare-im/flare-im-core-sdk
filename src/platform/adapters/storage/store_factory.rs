//! Storage adapter factory.

use std::sync::Arc;

use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::persistence::in_memory_empty_im_provider;
use crate::platform::ports::storage::SecureKeyStore;
use crate::platform::runtime::{RuntimeConfig, StorageKind};
use crate::shared::error::{ErrorCode, FlareError, Result};

pub async fn open_store_from_runtime_config(config: &RuntimeConfig) -> Result<StoreProvider> {
    open_store_from_runtime_config_with_secure_key_store(config, None).await
}

pub async fn open_store_from_runtime_config_with_secure_key_store(
    config: &RuntimeConfig,
    secure_key_store: Option<Arc<dyn SecureKeyStore>>,
) -> Result<StoreProvider> {
    match config.storage.kind {
        StorageKind::Memory => Ok(in_memory_empty_im_provider()),
        StorageKind::Sqlite => open_sqlite_stores(config, secure_key_store).await,
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
async fn open_sqlite_stores(
    config: &RuntimeConfig,
    secure_key_store: Option<Arc<dyn SecureKeyStore>>,
) -> Result<StoreProvider> {
    use std::path::PathBuf;

    use crate::shared::util::{
        default_sdk_data_root, open_sqlite_store_for_user,
        open_sqlite_store_for_user_with_secure_key_store,
    };

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
    if config.storage.encryption.is_enabled() {
        let secure_key_store = secure_key_store.ok_or_else(|| {
            FlareError::localized(
                ErrorCode::ConfigurationError,
                "SQLite encryption requires a SecureKeyStore",
            )
        })?;
        return open_sqlite_store_for_user_with_secure_key_store(
            &base_data_dir,
            user_id,
            &config.storage.namespace,
            &config.storage.tenant_id,
            config.storage.encryption.key_name.as_deref(),
            secure_key_store.as_ref(),
        )
        .await;
    }
    open_sqlite_store_for_user(&base_data_dir, user_id).await
}

#[cfg(not(feature = "lifecycle-sqlite"))]
async fn open_sqlite_stores(
    _config: &RuntimeConfig,
    _secure_key_store: Option<Arc<dyn SecureKeyStore>>,
) -> Result<StoreProvider> {
    Err(FlareError::localized(
        ErrorCode::ConfigurationError,
        "SQLite storage requires the lifecycle-sqlite feature",
    ))
}

#[cfg(all(test, feature = "lifecycle-sqlite"))]
mod tests {
    use super::*;
    use crate::client::SdkConfig;
    use crate::platform::runtime::{
        MediaRuntimeConfig, MediaRuntimeKind, PlatformKind, StorageConfig, StorageEncryptionConfig,
    };

    fn encrypted_sqlite_config() -> RuntimeConfig {
        RuntimeConfig {
            platform: PlatformKind::Native,
            sdk: SdkConfig::default(),
            storage: StorageConfig {
                kind: StorageKind::Sqlite,
                namespace: "flare".to_string(),
                tenant_id: "0".to_string(),
                user_id: "alice".to_string(),
                path: None,
                encryption: StorageEncryptionConfig::required(),
            },
            media: MediaRuntimeConfig {
                kind: MediaRuntimeKind::Native,
                cache_namespace: None,
            },
        }
    }

    #[tokio::test]
    async fn encrypted_sqlite_runtime_requires_secure_key_store() {
        let err = match open_store_from_runtime_config_with_secure_key_store(
            &encrypted_sqlite_config(),
            None,
        )
        .await
        {
            Ok(_) => panic!("missing secure key store must fail"),
            Err(err) => err,
        };

        assert_eq!(err.code(), Some(ErrorCode::ConfigurationError));
    }
}
