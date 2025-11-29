
你现在是一位具备 10 年以上经验的顶级 IM (Instant Messaging) 领域专家、跨平台客户端 SDK 架构师、分布式系统架构师，并深度熟悉微信、飞书、WhatsApp、Discord 的 IM 架构体系与 SDK 内核设计。你同时具备多端（Android/iOS/HarmonyOS/Web/PC）SDK 研发经验，熟悉 Flutter/React Native/Capacitor/UniApp 等跨平台技术的扩展模型。

# Flare IM Client SDK

跨平台的即时通讯客户端SDK，支持Web、PC桌面、Android、iOS、鸿蒙等平台。

## 功能特性

- ✅ 跨平台支持
- ✅ 长连接管理（WebSocket/QUIC，支持协议竞速）
- ✅ 消息收发
- ✅ 消息同步（基于 seq）
- ✅ 会话管理
- ✅ 本地存储（SQLite/IndexedDB）
- ✅ 媒体上传/下载（HTTP）

## 快速开始

```rust
use flare_im_core_sdk::{FlareIMClient, ClientConfig};
use flare_core::common::config_types::TransportProtocol;
use std::time::Duration;

let config = ClientConfig::builder()
    .server_url("wss://im.example.com")
    .media_base_url("https://media.example.com")
    .protocols(vec![
        TransportProtocol::QUIC,      // 优先级1（最快）
        TransportProtocol::WebSocket,  // 优先级2（备用）
    ])
    .race_timeout(Duration::from_secs(5))
    .user_id("user_123")
    .device_id("device_456")
    .build()?;

let client = FlareIMClient::new(config).await?;
```

## 文档

- [架构设计文档](./doc/01-客户端SDK架构设计.md)
- [客户端使用指南](./doc/02-客户端使用指南.md) ⭐ **推荐阅读**
- [代码完整性检查报告](./doc/03-代码完整性检查报告.md)
- [实施计划](./doc/plan/README.md)

## License

MIT
# Flare IM Core SDK 使用示例

## 消息创建与发送

```rust
use flare_im_core_sdk::{FlareIMClient, ClientConfig, MessageBuilder, SendOptions};
use flare_core::common::protocol::Reliability;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ClientConfig::builder()
        .server_url("wss://im.example.com")
        .user_id("user_123")
        .device_id("device_456")
        .build()?;

    let client = FlareIMClient::new(config).await?;
    client.login("user_123", "token").await?;

    let message = MessageBuilder::new()
        .session_id("session_1".to_string())
        .sender_id("user_123".to_string())
        .priority(5)
        .text("Hello, World!".to_string())
        .build();

    let opts = SendOptions { reliability: Reliability::AtLeastOnce, priority: Some(5) };
    let id = client.message_service().send_message_with_options("session_1", message.content.unwrap(), opts).await?;
    println!("sent: {}", id);
    Ok(())
}
```

## 会话设置

```rust
client.session_service().set_pinned("session_1", true).await?;
client.session_service().set_muted("session_1", false).await?;
client.session_service().set_alert_mode("session_1", "mentions").await?;
```
