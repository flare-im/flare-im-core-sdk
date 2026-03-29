use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

use crate::error::Result;

/// 与 flare-server-core `TokenClaims` 保持一致的 JWT Claims
#[derive(Debug, Clone, Serialize)]
struct TokenClaims {
    sub: String,
    iss: String,
    exp: usize,
    iat: usize,
    jti: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant_id: Option<String>,
}

static JTI_SEQ: AtomicU64 = AtomicU64::new(0);

fn generate_jti() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = JTI_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{seq:04x}")
}

/// 生成测试用 JWT token（HS256）
///
/// 产出的 token 与 `flare-server-core::TokenService::generate_token` 格式一致，
/// 可直接用于 flare-im-core access-gateway 的认证。
///
/// # 参数
/// - `secret` — HMAC 密钥，需与服务端 `token_secret` 一致
/// - `issuer` — 签发者，需与服务端 `token_issuer` 一致
/// - `user_id` — 用户 ID，写入 `sub` 字段
/// - `ttl_secs` — token 有效期（秒）
/// - `device_id` — 可选设备 ID
/// - `tenant_id` — 可选租户 ID
///
/// # 示例
///
/// ```
/// use flare_im_core_sdk::util::generate_test_token;
///
/// let token = generate_test_token(
///     "insecure-secret",
///     "flare-im-core",
///     "user_001",
///     3600,
///     None,
///     Some("default"),
/// ).unwrap();
/// ```
pub fn generate_test_token(
    secret: &str,
    issuer: &str,
    user_id: &str,
    ttl_secs: u64,
    device_id: Option<&str>,
    tenant_id: Option<&str>,
) -> Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| {
        crate::error::FlareError::localized(
            crate::error::ErrorCode::ConfigurationError,
            format!("system time error: {e}"),
        )
    })?;

    let iat = now.as_secs() as usize;
    let exp = (now.as_secs() + ttl_secs.max(60)) as usize;

    let claims = TokenClaims {
        sub: user_id.to_string(),
        iss: issuer.to_string(),
        exp,
        iat,
        jti: generate_jti(),
        device_id: device_id.map(|s| s.to_string()),
        tenant_id: tenant_id.map(|s| s.to_string()),
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| {
        crate::error::FlareError::localized(
            crate::error::ErrorCode::ConfigurationError,
            format!("token encode failed: {e}"),
        )
    })
}
