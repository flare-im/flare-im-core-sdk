//! 生成测试用的 JWT Token
//!
//! 用于开发测试，生成符合服务器要求的 JWT token
//!
//! ## 运行方式
//!
//! ```bash
//! # 生成 token
//! cargo run --example generate_test_token -- user-alice
//!
//! # 注意：secret 已写死为 "insecure-secret"（与服务器配置一致）
//! ```

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String, // user_id
    iss: String, // issuer
    exp: usize,  // expiration time
    iat: usize,  // issued at
    jti: String, // JWT ID
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: {} <user_id>", args[0]);
        eprintln!("示例: {} user-alice", args[0]);
        std::process::exit(1);
    }

    let user_id = &args[1];

    // 使用固定的 secret（与服务器配置一致）
    // 注意：生产环境必须使用服务器颁发的有效 token，不要使用此测试 token
    let secret = "insecure-secret";

    // 使用固定的 issuer（与服务器配置一致）
    let issuer = "flare-im-core";

    // 计算过期时间（7天后）
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    let exp = now + 7 * 24 * 60 * 60; // 7 days

    // 生成 JWT ID
    let jti = Uuid::new_v4().to_string();

    let claims = Claims {
        sub: user_id.clone(),
        iss: issuer.to_string(),
        exp,
        iat: now,
        jti,
    };

    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_ref()),
    )?;

    println!("✅ 生成的 Token:");
    println!("{}", token);
    println!();
    println!("💡 使用方法:");
    println!("   export TOKEN=\"{}\"", token);
    println!(
        "   RUST_LOG=info MY_USER_ID={} CHAT_WITH=user-bob TOKEN=$TOKEN cargo run --example two_clients_chat",
        user_id
    );

    Ok(())
}
