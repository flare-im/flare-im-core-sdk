use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::prelude::*;

#[path = "common/dev_token.rs"]
mod dev_token;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let secret = dev_token::require()?;
    let client = IMClient::new();
    client
        .init(Some("e2e-unread-regression".into()), None)
        .await?;
    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: secret.clone(),
        issuer: "flare-im-core".to_string(),
        user_id: "unread_regression".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login(
            "unread_regression",
            Some(&token),
            LoginDbKind::Sqlite,
            |_, _| {},
        )
        .await?;

    let conversation = apis
        .conversation_api
        .get_one("unread_peer", &ConversationType::Single)
        .await?;
    apis.conversation_api
        .mark_read(&conversation.conversation_id, u64::MAX)
        .await?;
    let refreshed = apis
        .conversation_api
        .get(&conversation.conversation_id)
        .await?;
    println!("conversation after mark_read: {:?}", refreshed);
    Ok(())
}
