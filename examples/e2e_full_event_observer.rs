use flare_im_core_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = IMClient::new();
    client
        .init(Some("e2e-full-event-observer".into()), None)
        .await?;
    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: "insecure-secret".to_string(),
        issuer: "flare-im-core".to_string(),
        user_id: "full_event_observer".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login(
            "full_event_observer",
            Some(&token),
            LoginDbKind::Sqlite,
            |_, _| {},
        )
        .await?;

    let _any = client.on_any(|event| {
        println!("sdk event: {:?}", event);
    })?;
    let result = apis
        .capability_api
        .rtc_sfu_join_room("example_full", "call_full", "room_a", Some("speaker"), None)
        .await?;
    println!("sfu join: {:?}", result.data);
    Ok(())
}
