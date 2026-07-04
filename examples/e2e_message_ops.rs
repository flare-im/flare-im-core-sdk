use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = IMClient::new();
    client.init(Some("e2e-message-ops".into()), None).await?;
    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: "insecure-secret".to_string(),
        issuer: "flare-im-core".to_string(),
        user_id: "message_ops".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login("message_ops", Some(&token), LoginDbKind::Sqlite, |_, _| {})
        .await?;

    let conversation = apis
        .conversation_api
        .get_one("message_peer", &ConversationType::Single)
        .await?;
    let message = apis
        .message_build_api
        .create_text(
            &conversation.conversation_id,
            "hello message ops",
            false,
            &[],
        )
        .await?;
    let ack = apis.message_api.send_no_oss(message).await?;
    println!("sent: {:?}", ack);

    let list = apis
        .message_api
        .list(&conversation.conversation_id, 0, 20)
        .await?;
    println!("local messages: {}", list.len());
    Ok(())
}
