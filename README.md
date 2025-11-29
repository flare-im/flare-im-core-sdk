# Flare IM Client SDK

[![Rust](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

跨平台的即时通讯客户端 SDK，基于 Rust 2024 Edition 开发，支持 Web、PC 桌面、Android、iOS、鸿蒙等平台。

## ✨ 功能特性

### 核心功能

- ✅ **跨平台支持**：Web (WASM)、PC 桌面、Android (JNI)、iOS (C FFI)、鸿蒙
- ✅ **长连接管理**：基于 `flare-core` 的 WebSocket/QUIC 双协议支持，支持协议竞速
- ✅ **消息收发**：支持文本、图片、文件、语音、视频等多种消息类型
- ✅ **消息同步**：基于序列号 (seq) 的增量同步和全量同步
- ✅ **会话管理**：会话列表、会话信息、未读数管理
- ✅ **本地存储**：SQLite (桌面/移动端) / IndexedDB (Web) 持久化存储
- ✅ **媒体处理**：HTTP 上传/下载，支持进度回调
- ✅ **离线支持**：离线消息缓存、重连后自动同步
- ✅ **事件系统**：连接、消息、会话、同步等事件通知
- ✅ **扩展机制**：支持自定义消息类型和处理逻辑

### 高级特性

- 🚀 **协议竞速**：同时尝试多个协议，选择最快的连接
- 🔄 **自动重连**：智能重连策略，支持指数退避
- 📦 **消息队列**：优先级队列，支持批量发送
- 🔐 **加密支持**：AES-GCM 端到端加密（可选）
- 📊 **状态管理**：消息状态追踪（发送中/已发送/已读/失败）
- 🎯 **扩展系统**：可扩展的用户信息、会话信息填充机制

## 🚀 快速开始

### 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
flare-im-core-sdk = { path = "../flare-im-core-sdk" }
flare-core = { path = "../flare-core" }
flare-proto = { path = "../flare-proto" }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
```

### 最小示例

```rust
use flare_im_core_sdk::{FlareIMClient, ClientConfig};
use flare_core::common::config_types::TransportProtocol;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 创建配置
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
        .token("your_token")
        .build()?;

    // 2. 创建客户端
    let client = FlareIMClient::new(config).await?;

    // 3. 登录
    let login_result = client.login("user_123", "your_token").await?;
    println!("登录成功: {:?}", login_result);

    // 4. 发送消息
    use flare_proto::{MessageContent, TextContent};
    
    let message_id = client.send_message(
        "session_123",
        MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
                TextContent {
                    text: "Hello, World!".to_string(),
                    mentions: vec![],
                }
            )),
        },
    ).await?;
    println!("消息已发送: {}", message_id);

    Ok(())
}
```

## 📖 使用示例

### 完整功能示例

查看 [examples/complete_client.rs](./examples/complete_client.rs) 了解完整的使用示例，包括：

- 连接和认证
- 消息发送和接收
- 会话管理
- 消息同步
- 事件监听
- 错误处理和恢复

### 事件监听

```rust
use flare_im_core_sdk::{Event, ConnectionEvent, MessageEvent};

let event_bus = client.event_bus();
let mut event_rx = event_bus.subscribe();

tokio::spawn(async move {
    while let Ok(event) = event_rx.recv().await {
        match event {
            Event::Connection(ConnectionEvent::Connected { protocol }) => {
                println!("连接成功，协议: {:?}", protocol);
            }
            Event::Message(MessageEvent::MessageReceived { message_id, session_id }) => {
                println!("收到消息: {} (会话: {})", message_id, session_id);
            }
            _ => {}
        }
    }
});
```

### 会话管理

```rust
// 获取会话列表
let sessions = client.get_sessions(flare_im_core_sdk::SessionFilter::default()).await?;

// 设置会话属性
client.session_service().set_pinned("session_1", true).await?;
client.session_service().set_muted("session_1", false).await?;
client.session_service().set_alert_mode("session_1", "mentions").await?;
```

### 消息同步

```rust
// 全量同步（首次登录）
let sync_result = client.sync_service().full_sync().await?;
println!("同步完成: {} 个会话, {} 条消息", 
    sync_result.sessions.len(), 
    sync_result.messages.len()
);

// 增量同步
let cursor = client.sync_service().get_sync_cursor().await?;
let sync_result = client.sync_service().incremental_sync(&cursor).await?;
```

### 高级配置

```rust
use std::collections::HashMap;
use std::time::Duration;

let mut protocol_urls = HashMap::new();
protocol_urls.insert(TransportProtocol::WebSocket, "ws://localhost:60051/ws".to_string());
protocol_urls.insert(TransportProtocol::QUIC, "quic://localhost:60052".to_string());

let config = ClientConfig::builder()
    .server_url("wss://im.example.com")
    .protocols(vec![TransportProtocol::QUIC, TransportProtocol::WebSocket])
    .protocol_urls(protocol_urls)
    .race_timeout(Duration::from_secs(5))
    .connect_timeout(15)
    .heartbeat_interval(30)
    .auto_reconnect(true)
    .max_reconnect_attempts(10)
    .user_id("user_123")
    .device_id("device_456")
    .device_platform(flare_im_core_sdk::DevicePlatform::Desktop)
    .token("your_token")
    .build()?;
```

## 🏗️ 架构设计

### 核心模块

```
flare-im-core-sdk/
├── client.rs          # 客户端主入口
├── config.rs          # 配置管理
├── connection/        # 连接管理（基于 flare-core）
├── protocol/          # 协议层（Frame 构建/解析）
├── service/           # 业务服务层
│   ├── message/       # 消息服务
│   ├── session.rs     # 会话服务
│   └── sync.rs        # 同步服务
├── storage/           # 存储抽象层
│   ├── sqlite.rs      # SQLite 实现
│   └── indexeddb.rs   # IndexedDB 实现（Web）
├── model/             # 数据模型
├── event/             # 事件系统
├── extension/         # 扩展系统（可选）
└── platform/          # 平台适配层
```

### 设计原则

1. **基于 flare-core**：长连接管理完全依赖 `flare-core` 框架
2. **基于 flare-proto**：消息和会话结构直接使用 `flare-proto` 定义
3. **跨平台抽象**：通过平台适配层实现多平台支持
4. **可扩展性**：通过扩展系统支持自定义功能
5. **类型安全**：充分利用 Rust 类型系统，避免运行时错误

## 📚 文档

### 核心文档

- [重构架构设计](./REFACTOR_ARCHITECTURE.md) - 重构后的架构说明
- [重构计划](./REFACTOR_PLAN.md) - 重构实施计划

> 更多详细文档请查看项目源码中的注释和示例代码。

## 🔧 平台支持

### 已支持平台

| 平台 | 状态 | 存储后端 | 说明 |
|------|------|----------|------|
| Web (WASM) | ✅ | IndexedDB | 通过 `wasm-bindgen` 编译为 WASM |
| PC 桌面 | ✅ | SQLite | 支持 Windows、macOS、Linux |
| Android | 🚧 | SQLite | 通过 JNI 绑定（开发中） |
| iOS | 🚧 | SQLite | 通过 C FFI 绑定（计划中） |
| 鸿蒙 | 📋 | SQLite | 计划支持 |

### 编译目标

```bash
# Web (WASM)
cargo build --target wasm32-unknown-unknown

# PC 桌面
cargo build --target x86_64-unknown-linux-gnu
cargo build --target x86_64-pc-windows-msvc
cargo build --target x86_64-apple-darwin

# Android
cargo build --target aarch64-linux-android
```

## 🛠️ 开发

### 运行示例

```bash
# 完整功能示例
RUST_LOG=info cargo run --example complete_client

# 双客户端聊天示例
RUST_LOG=info cargo run --example two_clients_chat

# 指定服务器地址和用户ID
RUST_LOG=info SERVER_URL=ws://localhost:60051/ws USER_ID=user123 \
    cargo run --example complete_client
```

### 运行测试

```bash
# 单元测试
cargo test

# 集成测试
cargo test --test '*'

# 代码检查
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

### 功能特性

SDK 支持可选的功能特性：

```toml
[dependencies]
flare-im-core-sdk = { path = "../flare-im-core-sdk", features = ["extensions"] }
```

- `extensions` - 启用扩展系统（用户信息、会话信息扩展）
- `storage-tools` - 启用存储工具（调试用）

## 📦 依赖说明

### 核心依赖

- **flare-core** - 长连接框架（WebSocket/QUIC）
- **flare-proto** - 协议定义（消息、会话结构）
- **tokio** - 异步运行时
- **serde** - 序列化/反序列化

### 平台特定依赖

- **wasm-bindgen** / **web-sys** - Web 平台支持
- **sqlx** - SQLite 数据库（非 WASM 平台）
- **reqwest** - HTTP 客户端（媒体上传/下载）

## 🤝 贡献

欢迎贡献！请遵循以下步骤：

1. Fork 本仓库
2. 创建功能分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'feat: add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 开启 Pull Request

### 代码规范

- 遵循 Rust 2024 Edition 规范
- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码
- 所有公共 API 必须有文档注释
- 提交信息遵循 [Conventional Commits](https://www.conventionalcommits.org/) 规范

## 📄 License

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。

## 🔗 相关项目

- [flare-core](../flare-core/) - 长连接框架
- [flare-proto](../flare-proto/) - 协议定义
- [flare-im-core](../flare-im-core/) - 服务端实现

## 📞 支持

如有问题或建议，请：

- 提交 [Issue](https://github.com/flare-team/flare-im-core-sdk/issues)
- 查看示例代码：`examples/` 目录
- 查看源码注释和文档

---

**Flare IM Client SDK** - 让即时通讯开发更简单 🚀
