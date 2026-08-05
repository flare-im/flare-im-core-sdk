//! 端到端加密的**参考实现**：X25519 ECDH → HKDF → XChaCha20-Poly1305。
//!
//! E2EE 管线（[`crate::extension::encryption`]）本身是密码学无关的：它负责把明文
//! 内容换成密文信封、在收端换回来，具体怎么加密由注入的 [`ContentCodec`] 决定。
//! 这个模块补上那块此前缺失的拼图 —— 在此之前仓库里唯一的 `ContentCodec` 实现是
//! 测试用的 `PrefixCodec`（只加个前缀，根本不加密），也就是说「支持 E2EE」在
//! 开箱状态下没有任何一行真正的密码学。
//!
//! # 这个实现做什么、不做什么
//!
//! **做**：给定双方的 X25519 密钥，派生一个会话密钥，用 XChaCha20-Poly1305 对
//! 消息内容做 AEAD 加解密。服务端全程只看得到密文与信封属性。
//!
//! **不做**：前向保密与后向恢复（Double Ratchet）、多设备密钥同步、预共享密钥
//! 的分发与轮换。这些属于密钥管理，接口在 [`crate::extension::encryption::E2eeKeyManager`]，
//! 由接入方按自己的身份体系实现 —— 那正是开源边界之外的部分。
//!
//! **所以：这是一个可用于评估与自建的参考实现，不是拿来即用的生产级 E2EE 协议栈。**
//! 把它当成「管线是通的、密码学接得上」的证据，以及自己实现 codec 时的模板。
//!
//! # 用法
//!
//! ```ignore
//! let codec = Arc::new(X25519AeadCodec::new(my_secret, peer_public)?);
//! let interceptor = ContentEncryptionInterceptor::new(codec, resolver);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::extension::{ContentCodec, ExtensionContent};
use crate::shared::error::{ErrorCode, FlareError, Result};

/// 本 codec 的命名空间与内容类型 —— 收端据此找到对应实现。
pub const E2EE_CODEC_NAMESPACE: &str = "flare.e2ee.x25519-xchacha20poly1305";
pub const E2EE_CODEC_CONTENT_TYPE: &str = "application/vnd.flare.e2ee.v1";

/// HKDF 的 info 串。写死而非可配：两端必须一致，做成参数只会多一个对不上的机会。
const HKDF_INFO: &[u8] = b"flare-im/e2ee/v1/content-key";

/// XChaCha20 的 nonce 是 24 字节 —— 比 ChaCha20 的 12 字节宽得多，
/// 因此可以安全地随机生成而不必维护计数器（12 字节随机 nonce 在同一密钥下
/// 发够多消息就会有碰撞风险，这正是这里选 X 变体的原因）。
const NONCE_LEN: usize = 24;

/// X25519 + XChaCha20-Poly1305 的内容编解码器。
pub struct X25519AeadCodec {
    key: XChaCha20Poly1305,
}

impl X25519AeadCodec {
    /// 用本端私钥与对端公钥协商出会话密钥。
    ///
    /// 双方各自调用（自己的私钥 + 对方的公钥）会得到同一把密钥 —— 这是 ECDH 的
    /// 基本性质，也是这个 codec 不需要传输任何密钥材料的原因。
    pub fn new(my_secret: StaticSecret, peer_public: PublicKey) -> Result<Self> {
        let shared = my_secret.diffie_hellman(&peer_public);
        Self::from_shared_secret(shared.as_bytes())
    }

    /// 从已有的共享密钥派生（供接入方自带密钥协商时复用 AEAD 部分）。
    pub fn from_shared_secret(shared: &[u8]) -> Result<Self> {
        // 直接拿 ECDH 输出当对称密钥是常见错误：它不是均匀分布的，
        // 且没有绑定用途。过一遍 HKDF 才得到可安全使用的密钥材料。
        let hk = Hkdf::<Sha256>::new(None, shared);
        let mut okm = [0u8; 32];
        hk.expand(HKDF_INFO, &mut okm).map_err(|_| {
            FlareError::localized(ErrorCode::InternalError, "E2EE HKDF expand failed")
        })?;
        let key = XChaCha20Poly1305::new_from_slice(&okm).map_err(|_| {
            FlareError::localized(ErrorCode::InternalError, "E2EE invalid AEAD key length")
        })?;
        Ok(Self { key })
    }

    /// 生成一对 X25519 密钥，供示例与自测使用。
    pub fn generate_keypair() -> (StaticSecret, PublicKey) {
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    /// 把 content_type 与 attributes 序列化成**自描述头**。
    ///
    /// 这个头以明文随密文一起走，同时充当 AEAD 的附加认证数据（AAD）：
    /// 不加密是因为收端得先读到它才知道内容类型；参与认证是因为不这样的话，
    /// 中间人可以在不碰密文的前提下改掉 content_type 或属性，而解密照样成功。
    ///
    /// 早先的写法是让 encode 从 `ExtensionContent` 现算 AAD —— 但 decode 手里
    /// 只有一串字节，无从重建同一个 AAD，于是自己加密的东西自己解不开。
    /// 把头写进载荷是唯一能让 `ContentCodec` 这个「字节进、字节出」的契约
    /// 自洽的做法。
    fn encode_header(content: &ExtensionContent) -> Vec<u8> {
        let mut keys: Vec<&String> = content.attributes.keys().collect();
        keys.sort(); // 顺序必须确定，否则两端算出的 AAD 不同
        let mut out = Vec::new();
        put_str(&mut out, &content.content_type);
        out.extend_from_slice(&(keys.len() as u32).to_be_bytes());
        for k in keys {
            put_str(&mut out, k);
            put_str(&mut out, &content.attributes[k]);
        }
        out
    }

    fn decode_header(mut buf: &[u8]) -> Result<(String, HashMap<String, String>)> {
        let content_type = take_str(&mut buf)?;
        let n = take_u32(&mut buf)? as usize;
        let mut attributes = HashMap::with_capacity(n);
        for _ in 0..n {
            let k = take_str(&mut buf)?;
            let v = take_str(&mut buf)?;
            attributes.insert(k, v);
        }
        Ok((content_type, attributes))
    }
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn take_u32(buf: &mut &[u8]) -> Result<u32> {
    if buf.len() < 4 {
        return Err(malformed());
    }
    let (head, rest) = buf.split_at(4);
    *buf = rest;
    Ok(u32::from_be_bytes([head[0], head[1], head[2], head[3]]))
}

fn take_str(buf: &mut &[u8]) -> Result<String> {
    let n = take_u32(buf)? as usize;
    if buf.len() < n {
        return Err(malformed());
    }
    let (head, rest) = buf.split_at(n);
    *buf = rest;
    String::from_utf8(head.to_vec()).map_err(|_| malformed())
}

fn malformed() -> FlareError {
    FlareError::localized(ErrorCode::InvalidParameter, "E2EE malformed payload")
}

impl ContentCodec for X25519AeadCodec {
    fn namespace(&self) -> &str {
        E2EE_CODEC_NAMESPACE
    }

    fn content_type(&self) -> &str {
        E2EE_CODEC_CONTENT_TYPE
    }

    fn encode(&self, content: &ExtensionContent) -> Result<Vec<u8>> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);
        let header = Self::encode_header(content);
        let ciphertext = self
            .key
            .encrypt(
                nonce,
                Payload {
                    msg: &content.payload,
                    aad: &header,
                },
            )
            .map_err(|_| FlareError::localized(ErrorCode::InternalError, "E2EE encrypt failed"))?;

        // 布局：nonce(24) || header_len(4) || header || ciphertext
        // nonce 与 header 都是明文 —— 前者不是秘密，后者收端必须先读到；
        // 两者都被 AEAD 认证覆盖，改动即失败。
        let mut out = Vec::with_capacity(NONCE_LEN + 4 + header.len() + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&(header.len() as u32).to_be_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    fn decode(&self, payload: &[u8]) -> Result<ExtensionContent> {
        if payload.len() <= NONCE_LEN + 4 {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "E2EE ciphertext too short",
            ));
        }
        let (nonce_bytes, mut rest) = payload.split_at(NONCE_LEN);
        let header_len = take_u32(&mut rest)? as usize;
        if rest.len() < header_len {
            return Err(malformed());
        }
        let (header, ciphertext) = rest.split_at(header_len);
        let (content_type, attributes) = Self::decode_header(header)?;

        let plaintext = self
            .key
            .decrypt(
                XNonce::from_slice(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: header,
                },
            )
            .map_err(|_| {
                FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "E2EE decrypt failed (wrong key or tampered payload)",
                )
            })?;

        Ok(ExtensionContent {
            content_type,
            payload: plaintext,
            attributes,
        })
    }
}

/// 便捷构造：双方共享密钥已知时直接拿到 `Arc<dyn ContentCodec>`。
pub fn shared_secret_codec(shared: &[u8]) -> Result<Arc<dyn ContentCodec>> {
    Ok(Arc::new(X25519AeadCodec::from_shared_secret(shared)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn content(payload: &[u8]) -> ExtensionContent {
        let mut attributes = HashMap::new();
        attributes.insert("conv".to_string(), "c-1".to_string());
        ExtensionContent {
            content_type: "text/plain".to_string(),
            payload: payload.to_vec(),
            attributes,
        }
    }

    #[test]
    fn ecdh_round_trips_content_type_and_attributes() {
        let (a_sec, a_pub) = X25519AeadCodec::generate_keypair();
        let (b_sec, b_pub) = X25519AeadCodec::generate_keypair();
        let alice = X25519AeadCodec::new(a_sec, b_pub).unwrap();
        let bob = X25519AeadCodec::new(b_sec, a_pub).unwrap();

        let original = content(b"hello");
        let sealed = alice.encode(&original).unwrap();
        // 双方从未传输过密钥，bob 用自己的私钥 + alice 的公钥就能解开
        let opened = bob.decode(&sealed).unwrap();

        assert_eq!(opened.payload, original.payload);
        // content_type 与 attributes 必须原样回来 —— 它们是被认证的头，
        // 不是可以丢掉的附属信息
        assert_eq!(opened.content_type, original.content_type);
        assert_eq!(opened.attributes, original.attributes);
    }

    #[test]
    fn tampered_attributes_are_rejected_even_with_intact_ciphertext() {
        // 属性以明文随载荷同行（收端要先读到才知道内容类型），所以必须被认证：
        // 否则中间人不碰密文、只改属性就能让接收方按错误的类型去解读内容。
        let (a_sec, a_pub) = X25519AeadCodec::generate_keypair();
        let (b_sec, b_pub) = X25519AeadCodec::generate_keypair();
        let alice = X25519AeadCodec::new(a_sec, b_pub).unwrap();
        let bob = X25519AeadCodec::new(b_sec, a_pub).unwrap();

        let mut sealed = alice.encode(&content(b"hello")).unwrap();
        // 头部里 "c-1" 的最后一个字节改掉（密文一个 bit 都没动）
        let pos = sealed
            .windows(3)
            .position(|w| w == b"c-1")
            .expect("属性值应以明文出现在头部");
        sealed[pos + 2] = b'2';
        assert!(bob.decode(&sealed).is_err());
    }

    #[test]
    fn ciphertext_is_not_plaintext_and_differs_per_call() {
        let (a_sec, _) = X25519AeadCodec::generate_keypair();
        let (_, b_pub) = X25519AeadCodec::generate_keypair();
        let codec = X25519AeadCodec::new(a_sec, b_pub).unwrap();
        let plain = ExtensionContent {
            content_type: "t".into(),
            payload: b"attack at dawn".to_vec(),
            attributes: Default::default(),
        };
        let c1 = codec.encode(&plain).unwrap();
        let c2 = codec.encode(&plain).unwrap();
        // 明文不得出现在密文里
        assert!(
            !c1.windows(14).any(|w| w == b"attack at dawn"),
            "明文泄漏在密文中"
        );
        // 随机 nonce ⇒ 同样的明文两次加密结果不同，否则可做重放/关联分析
        assert_ne!(c1, c2);
    }

    #[test]
    fn wrong_key_fails_to_decrypt() {
        let (a_sec, _) = X25519AeadCodec::generate_keypair();
        let (_, b_pub) = X25519AeadCodec::generate_keypair();
        let (c_sec, _) = X25519AeadCodec::generate_keypair();
        let (_, d_pub) = X25519AeadCodec::generate_keypair();
        let alice = X25519AeadCodec::new(a_sec, b_pub).unwrap();
        let stranger = X25519AeadCodec::new(c_sec, d_pub).unwrap();
        let plain = ExtensionContent {
            content_type: "t".into(),
            payload: b"secret".to_vec(),
            attributes: Default::default(),
        };
        let sealed = alice.encode(&plain).unwrap();
        assert!(stranger.decode(&sealed).is_err());
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let (a_sec, _) = X25519AeadCodec::generate_keypair();
        let (_, b_pub) = X25519AeadCodec::generate_keypair();
        let codec = X25519AeadCodec::new(a_sec, b_pub).unwrap();
        let plain = ExtensionContent {
            content_type: "t".into(),
            payload: b"transfer 100".to_vec(),
            attributes: Default::default(),
        };
        let mut sealed = codec.encode(&plain).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01; // 翻一个 bit
        // AEAD 的意义就在这里：篡改必须被发现，而不是解出一段乱码
        assert!(codec.decode(&sealed).is_err());
    }

    #[test]
    fn truncated_payload_is_rejected() {
        let (a_sec, _) = X25519AeadCodec::generate_keypair();
        let (_, b_pub) = X25519AeadCodec::generate_keypair();
        let codec = X25519AeadCodec::new(a_sec, b_pub).unwrap();
        assert!(codec.decode(&[0u8; 8]).is_err());
    }
}
