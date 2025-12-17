use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use anyhow::Result;

pub trait CryptoService: Send + Sync {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>>;
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>>;
}

pub struct NoopCrypto;

impl CryptoService for NoopCrypto {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        Ok(data.to_vec())
    }
}

pub struct AesCrypto {
    cipher: Aes256Gcm,
}

impl AesCrypto {
    pub fn new(key_bytes: &[u8]) -> Result<Self> {
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key_bytes);
        let cipher = Aes256Gcm::new(key);
        Ok(Self { cipher })
    }
}

impl CryptoService for AesCrypto {
    fn encrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(nonce, data)
            .map_err(|e| anyhow::anyhow!("AES-GCM encrypt failed: {:?}", e))?;
        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            return Err(anyhow::anyhow!("ciphertext too short"));
        }
        let (nonce_bytes, ct) = data.split_at(12);
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ct)
            .map_err(|e| anyhow::anyhow!("AES-GCM decrypt failed: {:?}", e))?;
        Ok(pt)
    }
}
