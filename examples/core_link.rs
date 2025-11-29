use flare_im_core_sdk::{FlareIMClient, ClientConfig, ClientConfigBuilder};
use flare_core::common::config_types::TransportProtocol;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut urls = HashMap::new();
    urls.insert(TransportProtocol::WebSocket, "ws://127.0.0.1:60051".to_string());
    urls.insert(TransportProtocol::QUIC, "quic://127.0.0.1:60052".to_string());

    let config = ClientConfig::builder()
        .server_url("ws://127.0.0.1:60051")
        .user_id("demo_user")
        .device_id("device_demo")
        .protocol_urls(urls)
        .protocols(vec![TransportProtocol::QUIC, TransportProtocol::WebSocket])
        .race_timeout(std::time::Duration::from_secs(10))
        .build()?;

    let client = FlareIMClient::new(config).await?;

    let _ = client.login("demo_user", "demo_token").await?;

    let content = flare_proto::MessageContent {
        content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
            flare_proto::TextContent { text: "Hello from SDK".to_string(), mentions: vec![] }
        )),
    };

    let _id = client.send_message("session_demo", content).await?;

    Ok(())
}
