use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = IMClient::new();
    client.init(Some("e2e-full-event-ops".into()), None).await?;
    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: "insecure-secret".to_string(),
        issuer: "flare-im-core".to_string(),
        user_id: "full_event_ops".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login(
            "full_event_ops",
            Some(&token),
            LoginDbKind::Sqlite,
            |_, _| {},
        )
        .await?;

    let conversation = apis
        .conversation_api
        .get_one("ops_peer", &ConversationType::Single)
        .await?;
    let message = apis
        .message_build_api
        .create_text(&conversation.conversation_id, "hello ops", false, &[])
        .await?;
    let client_msg_id = message.client_msg_id.clone();
    let _ = apis.message_api.send_no_oss(message).await?;

    apis.message_api
        .typing(&conversation.conversation_id, true)
        .await?;
    apis.message_api
        .add_reaction(&client_msg_id, "thumbs_up")
        .await?;
    apis.message_api
        .remove_reaction(&client_msg_id, "thumbs_up")
        .await?;
    println!("message ops issued for {client_msg_id}");
    Ok(())
}
