//! Storage ports.
//!
//! The concrete implementation can be SQLite, IndexedDB, memory, or a custom
//! host bridge. Core synchronization and projection logic must only use these
//! store contracts.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use rand::RngCore;
use tokio::sync::RwLock;

use crate::shared::error::{ErrorCode, FlareError, Result};

pub use crate::domain::{
    ConversationParticipantStore, ConversationReader, ConversationStore, ConversationWriter,
    MediaCacheAdmin, MediaCacheStore, MessageReader, MessageStore, MessageWriter,
    PendingSendReader, PendingSendWriter, SyncCursorReader, SyncCursorStore, SyncCursorWriter,
    UploadManifestStore, UserFileDownloadStore, UserReader, UserWriter,
};
pub use crate::infrastructure::persistence::StoreProvider;

/// AES-256/SQLCipher local database key size used by the SDK.
pub const LOCAL_DATABASE_KEY_BYTES: usize = 32;

/// Stable key descriptor used when asking a platform secure store for a secret.
///
/// The descriptor deliberately carries only routing metadata. The key material
/// itself is returned as [`SecureSecret`] and must never be logged or serialized.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SecureKeyDescriptor {
    namespace: String,
    tenant_id: String,
    user_id: String,
    purpose: String,
}

impl fmt::Debug for SecureKeyDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureKeyDescriptor")
            .field("namespace", &self.namespace)
            .field("tenant_id", &self.tenant_id)
            .field("user_id", &self.user_id)
            .field("purpose", &self.purpose)
            .finish()
    }
}

impl SecureKeyDescriptor {
    pub fn new(
        namespace: impl Into<String>,
        tenant_id: impl Into<String>,
        user_id: impl Into<String>,
        purpose: impl Into<String>,
    ) -> Self {
        Self {
            namespace: normalized_segment(namespace.into(), "flare-im-core-sdk"),
            tenant_id: normalized_segment(tenant_id.into(), "0"),
            user_id: normalized_segment(user_id.into(), "unknown"),
            purpose: normalized_segment(purpose.into(), "local_database.v1"),
        }
    }

    pub fn local_database(
        namespace: impl Into<String>,
        tenant_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self::new(namespace, tenant_id, user_id, "local_database.v1")
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    /// A stable, platform-friendly lookup key for Keychain/Keystore/DPAPI
    /// adapters that prefer a single string identifier.
    pub fn storage_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            sanitize_key_segment(&self.namespace),
            sanitize_key_segment(&self.tenant_id),
            sanitize_key_segment(&self.user_id),
            sanitize_key_segment(&self.purpose)
        )
    }
}

/// Redacted secret material returned by [`SecureKeyStore`].
#[derive(Clone, PartialEq, Eq)]
pub struct SecureSecret {
    bytes: Vec<u8>,
}

impl fmt::Debug for SecureSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureSecret")
            .field("len", &self.bytes.len())
            .field("bytes", &"<redacted>")
            .finish()
    }
}

impl SecureSecret {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        if bytes.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "secure secret must not be empty",
            ));
        }
        Ok(Self { bytes })
    }

    pub fn expose_secret(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Platform secure key store SPI.
///
/// Native adapters should back this with Android Keystore, iOS/macOS Keychain,
/// Windows DPAPI/Credential Locker, or an equivalent host-protected secret
/// store. The Rust core only owns key lifecycle policy; it does not persist
/// plaintext database keys itself.
#[async_trait]
pub trait SecureKeyStore: Send + Sync {
    async fn get_secret(&self, descriptor: &SecureKeyDescriptor) -> Result<Option<SecureSecret>>;

    async fn put_secret(
        &self,
        descriptor: &SecureKeyDescriptor,
        secret: SecureSecret,
    ) -> Result<()>;

    async fn delete_secret(&self, descriptor: &SecureKeyDescriptor) -> Result<()>;
}

/// Volatile implementation for tests and host integration scaffolding.
///
/// This is not a production secure store; dropping the value loses every key.
#[derive(Default)]
pub struct VolatileSecureKeyStore {
    secrets: RwLock<HashMap<String, SecureSecret>>,
}

impl VolatileSecureKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[async_trait]
impl SecureKeyStore for VolatileSecureKeyStore {
    async fn get_secret(&self, descriptor: &SecureKeyDescriptor) -> Result<Option<SecureSecret>> {
        Ok(self
            .secrets
            .read()
            .await
            .get(&descriptor.storage_key())
            .cloned())
    }

    async fn put_secret(
        &self,
        descriptor: &SecureKeyDescriptor,
        secret: SecureSecret,
    ) -> Result<()> {
        self.secrets
            .write()
            .await
            .insert(descriptor.storage_key(), secret);
        Ok(())
    }

    async fn delete_secret(&self, descriptor: &SecureKeyDescriptor) -> Result<()> {
        self.secrets.write().await.remove(&descriptor.storage_key());
        Ok(())
    }
}

/// Loads the local DB key from the platform store, generating and storing a
/// fresh random key on first use.
pub async fn load_or_create_local_database_key(
    key_store: &dyn SecureKeyStore,
    descriptor: &SecureKeyDescriptor,
) -> Result<SecureSecret> {
    if let Some(secret) = key_store.get_secret(descriptor).await? {
        validate_local_database_key(&secret)?;
        return Ok(secret);
    }

    let mut key = vec![0_u8; LOCAL_DATABASE_KEY_BYTES];
    rand::thread_rng().fill_bytes(&mut key);
    let secret = SecureSecret::new(key)?;
    key_store.put_secret(descriptor, secret.clone()).await?;
    Ok(secret)
}

pub fn validate_local_database_key(secret: &SecureSecret) -> Result<()> {
    let len = secret.expose_secret().len();
    if len != LOCAL_DATABASE_KEY_BYTES {
        return Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            format!(
                "local database key must be {} bytes, got {}",
                LOCAL_DATABASE_KEY_BYTES, len
            ),
        ));
    }
    Ok(())
}

fn normalized_segment(value: String, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn sanitize_key_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_database_key_is_generated_once_and_reused() {
        let store = VolatileSecureKeyStore::new();
        let descriptor = SecureKeyDescriptor::local_database("flare", "tenant-a", "alice");

        let first = load_or_create_local_database_key(&store, &descriptor)
            .await
            .expect("first key");
        let second = load_or_create_local_database_key(&store, &descriptor)
            .await
            .expect("second key");

        assert_eq!(first.expose_secret().len(), LOCAL_DATABASE_KEY_BYTES);
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn invalid_stored_database_key_is_rejected() {
        let store = VolatileSecureKeyStore::new();
        let descriptor = SecureKeyDescriptor::local_database("flare", "tenant-a", "alice");
        store
            .put_secret(&descriptor, SecureSecret::new(vec![1, 2, 3]).unwrap())
            .await
            .expect("store key");

        let err = load_or_create_local_database_key(&store, &descriptor)
            .await
            .expect_err("invalid stored key");

        assert_eq!(err.code(), Some(ErrorCode::ConfigurationError));
    }

    #[test]
    fn secure_secret_debug_redacts_material() {
        let secret = SecureSecret::new(vec![7; LOCAL_DATABASE_KEY_BYTES]).unwrap();
        let debug = format!("{secret:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("7, 7, 7"));
    }

    #[test]
    fn descriptor_storage_key_is_platform_friendly() {
        let descriptor = SecureKeyDescriptor::new("flare im", "tenant/a", "user:b", "local db");

        assert_eq!(
            descriptor.storage_key(),
            "flare_im:tenant_a:user_b:local_db"
        );
    }
}
