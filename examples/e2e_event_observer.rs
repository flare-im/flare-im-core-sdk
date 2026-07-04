use flare_im_core_sdk::model::conversation::ConversationType;
use flare_im_core_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = IMClient::new();
    client.init(Some("e2e-event-observer".into()), None).await?;
    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: "insecure-secret".to_string(),
        issuer: "flare-im-core".to_string(),
        user_id: "event_observer".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login(
            "event_observer",
            Some(&token),
            LoginDbKind::Sqlite,
            |_, _| {},
        )
        .await?;

    let _messages = client.on_message(|message| {
        println!(
            "message {} seq={}",
            message.client_msg_id, message.conversation_seq
        );
    })?;
    let conversation = apis
        .conversation_api
        .get_one("event_peer", &ConversationType::Single)
        .await?;
    let message = apis
        .message_build_api
        .create_text(&conversation.conversation_id, "hello events", false, &[])
        .await?;
    let ack = apis.message_api.send_no_oss(message).await?;
    println!("send ack: {:?}", ack);
    Ok(())
}
