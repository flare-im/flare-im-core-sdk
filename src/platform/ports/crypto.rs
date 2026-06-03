//! Crypto ports.
//!
//! Stable IM semantics should not depend on a concrete crypto backend.
//! Browser adapters can use WebCrypto; native adapters can use OS or Rust
//! crypto providers.

use async_trait::async_trait;

use crate::shared::error::Result;

#[async_trait]
pub trait CryptoPort: Send + Sync {
    async fn random_bytes(&self, len: usize) -> Result<Vec<u8>>;
    async fn sha256(&self, data: &[u8]) -> Result<[u8; 32]>;
}
