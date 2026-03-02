//! 工具模块
//!
//! 提供常用工具函数，包括会话 ID 生成等

// 重新导出 flare-core 的会话 ID 生成函数，方便 SDK 内部使用
// 注意：flare-core 已经通过 lib.rs 重新导出了这些函数
pub use flare_core::{
    generate_single_chat_conversation_id,
    generate_group_conversation_id,
    generate_ai_conversation_id,
    generate_customer_conversation_id,
    generate_system_conversation_id,
    generate_temp_conversation_id,
    validate_conversation_id,
    extract_conversation_type,
    is_single_chat_conversation,
    is_group_chat_conversation,
    ConversationType,
};

/// 生成测试用的 JWT Token (用于开发环境)
/// 注意：此 Token 使用不安全的密钥，仅供测试使用
pub fn generate_test_token(user_id: &str) -> anyhow::Result<String> {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use serde::{Deserialize, Serialize};
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    #[derive(Debug, Serialize, Deserialize)]
    struct Claims {
        sub: String,
        iss: String,
        exp: usize,
        iat: usize,
        jti: String,
    }

    let secret = "insecure-secret";
    let issuer = "flare-im-core";

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    let exp = now + 7 * 24 * 60 * 60;

    let jti = Uuid::new_v4().to_string();

    let claims = Claims {
        sub: user_id.to_string(),
        iss: issuer.to_string(),
        exp,
        iat: now,
        jti,
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| anyhow::anyhow!("Failed to generate token: {}", e))?;

    Ok(token)
}
