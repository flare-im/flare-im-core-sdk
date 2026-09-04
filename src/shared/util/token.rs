use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;

use crate::shared::error::{ErrorCode, FlareError, Result};

type HmacSha256 = Hmac<Sha256>;

/// 与 Flare IM 接入侧约定的 JWT Claims（字段与网关校验一致）
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

/// Core access JWT signing configuration.
///
/// This helper is intended for local gateways, examples, and controlled test
/// harnesses that need a gateway-compatible HS256 token. Signing material and
/// expiry must be supplied explicitly by the caller.
#[derive(Debug, Clone)]
pub struct CoreTokenConfig {
    pub secret: String,
    pub issuer: String,
    pub user_id: String,
    pub ttl_secs: u64,
    pub device_id: Option<String>,
    pub tenant_id: Option<String>,
}

fn generate_jti() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let ts = js_sys::Date::now() as u64;
        let seq = JTI_SEQ.fetch_add(1, Ordering::Relaxed);
        return format!("{ts:x}-{seq:04x}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = JTI_SEQ.fetch_add(1, Ordering::Relaxed);
        format!("{ts:x}-{seq:04x}")
    }
}

pub(crate) fn now_unix_secs() -> Result<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok((js_sys::Date::now() / 1000.0) as u64)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|e| {
                crate::shared::error::FlareError::localized(
                    crate::shared::error::ErrorCode::ConfigurationError,
                    format!("system time error: {e}"),
                )
            })
    }
}

fn base64url_json<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|e| {
        crate::shared::error::FlareError::localized(
            crate::shared::error::ErrorCode::ConfigurationError,
            format!("token json encode failed: {e}"),
        )
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn sign_hs256(secret: &str, signing_input: &str) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| {
        crate::shared::error::FlareError::localized(
            crate::shared::error::ErrorCode::ConfigurationError,
            format!("token hmac key error: {e}"),
        )
    })?;
    mac.update(signing_input.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn require_non_empty<'a>(field: &str, value: &'a str) -> Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            format!("core token {field} is required"),
        ));
    }
    Ok(trimmed)
}

/// 生成 Flare IM Core 接入 JWT token（HS256）。
///
/// 产出的 token 与服务端签发的 access token 字段布局一致，可直接用于
/// flare-im-core access-gateway 的认证。调用方必须显式提供签名 secret、
/// issuer、user_id 与过期时间，避免测试默认值进入真实集成路径。
///
/// Native 与 WASM 共用同一实现（不依赖 `jsonwebtoken`）。
pub fn generate_core_token(config: &CoreTokenConfig) -> Result<String> {
    let secret = require_non_empty("secret", &config.secret)?;
    let issuer = require_non_empty("issuer", &config.issuer)?;
    let user_id = require_non_empty("user_id", &config.user_id)?;
    if config.ttl_secs == 0 {
        return Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            "core token ttl_secs must be greater than zero",
        ));
    }

    let now = now_unix_secs()?;
    let iat = now as usize;
    let exp = (now + config.ttl_secs.max(60)) as usize;

    let claims = TokenClaims {
        sub: user_id.to_string(),
        iss: issuer.to_string(),
        exp,
        iat,
        jti: generate_jti(),
        device_id: config
            .device_id
            .as_deref()
            .map(str::trim)
            .and_then(|value| {
                if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            }),
        tenant_id: config
            .tenant_id
            .as_deref()
            .map(str::trim)
            .and_then(|value| {
                if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            }),
    };

    let header = base64url_json(&serde_json::json!({
        "alg": "HS256",
        "typ": "JWT",
    }))?;
    let payload = base64url_json(&claims)?;
    let signing_input = format!("{header}.{payload}");
    let signature = sign_hs256(secret, &signing_input)?;
    Ok(format!("{signing_input}.{signature}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;

    fn decode_payload(token: &str) -> serde_json::Value {
        let payload = token.split('.').nth(1).expect("jwt payload");
        let bytes = B64.decode(payload).expect("base64 payload");
        serde_json::from_slice(&bytes).expect("json payload")
    }

    #[test]
    fn generates_hs256_jwt_with_gateway_claims() {
        let token = generate_core_token(&CoreTokenConfig {
            secret: "gateway-secret".to_string(),
            issuer: "flare-im-core".to_string(),
            user_id: "alice".to_string(),
            ttl_secs: 3600,
            device_id: None,
            tenant_id: Some("0".to_string()),
        })
        .expect("token");
        assert_eq!(token.matches('.').count(), 2);
        let payload = decode_payload(&token);
        assert_eq!(payload["sub"], "alice");
        assert_eq!(payload["iss"], "flare-im-core");
        assert_eq!(payload["tenant_id"], "0");
        assert!(payload["exp"].as_u64().unwrap() > payload["iat"].as_u64().unwrap());
    }

    #[test]
    fn omits_optional_claims_when_absent() {
        let token = generate_core_token(&CoreTokenConfig {
            secret: "secret".to_string(),
            issuer: "issuer".to_string(),
            user_id: "bob".to_string(),
            ttl_secs: 120,
            device_id: None,
            tenant_id: None,
        })
        .expect("token");
        let payload = decode_payload(&token);
        assert!(payload.get("tenant_id").is_none());
        assert!(payload.get("device_id").is_none());
    }

    #[test]
    fn rejects_missing_signing_config() {
        let err = generate_core_token(&CoreTokenConfig {
            secret: " ".to_string(),
            issuer: "issuer".to_string(),
            user_id: "alice".to_string(),
            ttl_secs: 3600,
            device_id: None,
            tenant_id: None,
        })
        .expect_err("secret must be explicit");

        assert_eq!(
            err.code(),
            Some(crate::shared::error::ErrorCode::ConfigurationError)
        );
    }
}
