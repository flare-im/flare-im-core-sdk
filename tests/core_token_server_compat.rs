//! 客户端本地签发的接入 token 必须能过服务端网关的校验。
//!
//! 网关侧是 flare-server-core `flare-core-infra/src/auth/token.rs` 的 `TokenService::decode_claims`：
//! `Validation::new(HS256)` + `set_issuer([issuer])`，其余取 jsonwebtoken 默认（校验 exp，60s leeway）。
//! 这里照同一套规则解码 SDK 产出的 token。生产上「只输 user id + 签名密钥就能登录」
//! 的链路，除了密钥值本身，其余环节（算法/头/声明布局/签发者）全靠这条测试守住。

use flare_im_core_sdk::prelude::{generate_core_token, CoreTokenConfig};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

/// 与服务端 `TokenClaims` 字段一一对应（服务端按名字反序列化，多一个少一个都算不一致）。
#[derive(Debug, Deserialize)]
struct ServerClaims {
    sub: String,
    iss: String,
    exp: usize,
    iat: usize,
    jti: String,
    device_id: Option<String>,
    tenant_id: Option<String>,
}

fn server_validation(issuer: &str) -> Validation {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[issuer]);
    validation
}

fn mint(secret: &str, issuer: &str, user: &str, tenant: Option<&str>, device: Option<&str>) -> String {
    generate_core_token(&CoreTokenConfig {
        secret: secret.to_string(),
        issuer: issuer.to_string(),
        user_id: user.to_string(),
        ttl_secs: 3600,
        device_id: device.map(str::to_string),
        tenant_id: tenant.map(str::to_string),
    })
    .expect("mint")
}

#[test]
fn client_minted_token_passes_server_validation() {
    let secret = "a-strong-shared-secret-with-more-than-32-bytes!";
    let token = mint(secret, "flare-im-core", "123", Some("0"), None);

    let data = decode::<ServerClaims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &server_validation("flare-im-core"),
    )
    .expect("服务端同规则校验必须通过");

    assert_eq!(data.header.alg, Algorithm::HS256);
    assert_eq!(data.claims.sub, "123");
    assert_eq!(data.claims.iss, "flare-im-core");
    assert_eq!(data.claims.tenant_id.as_deref(), Some("0"));
    assert_eq!(data.claims.device_id, None);
    assert!(!data.claims.jti.is_empty());
    assert!(data.claims.exp > data.claims.iat);
    assert_eq!(data.claims.exp - data.claims.iat, 3600);
}

#[test]
fn device_id_survives_round_trip() {
    let secret = "another-strong-shared-secret-with-32-bytes!!";
    let token = mint(secret, "flare-im-core", "u1", Some("t9"), Some("dev-abc"));
    let data = decode::<ServerClaims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &server_validation("flare-im-core"),
    )
    .expect("带 device_id 也必须通过");
    assert_eq!(data.claims.device_id.as_deref(), Some("dev-abc"));
    assert_eq!(data.claims.tenant_id.as_deref(), Some("t9"));
}

#[test]
fn wrong_secret_is_rejected_like_the_gateway_does() {
    let token = mint("the-right-secret-the-server-holds-32-bytes!!", "flare-im-core", "123", Some("0"), None);
    let err = decode::<ServerClaims>(
        &token,
        &DecodingKey::from_secret(b"definitely-wrong-secret-for-path-test"),
        &server_validation("flare-im-core"),
    )
    .expect_err("密钥不一致必须被拒——这就是生产上「Token 验证失败」那条日志");
    assert!(matches!(err.kind(), jsonwebtoken::errors::ErrorKind::InvalidSignature), "{err:?}");
}

#[test]
fn wrong_issuer_is_rejected() {
    let secret = "a-strong-shared-secret-with-more-than-32-bytes!";
    let token = mint(secret, "someone-else", "123", Some("0"), None);
    let err = decode::<ServerClaims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &server_validation("flare-im-core"),
    )
    .expect_err("签发者对不上必须被拒");
    assert!(matches!(err.kind(), jsonwebtoken::errors::ErrorKind::InvalidIssuer), "{err:?}");
}
