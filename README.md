# Flare IM Core SDK

[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

跨平台 IM 客户端 SDK（Rust）。覆盖长连接、消息、会话、同步与本地存储；提供 Rust API、C FFI（Flutter / 原生宿主）及 Tauri 绑定。

## 许可

[Apache License 2.0](LICENSE)。使用、修改与再分发须遵守许可条款，并保留 [NOTICE](NOTICE) 中的版权与来源说明。建议在产品的「关于 / 开源许可」页面注明：

> Includes software from Flare IM Core SDK (https://github.com/flare-im/flare-im-core-sdk)

底层 `flare-core` 为 MIT，见对应仓库许可文件。

## 能力概览

| 模块 | 说明 |
|------|------|
| 连接 | 基于 `flare-core` 的 WebSocket / QUIC |
| 协议 | 与 `flare-proto` 对齐的消息与会话模型 |
| 存储 | SQLite、IndexedDB 等可插拔后端 |
| 绑定 | `bindings/c`（FFI）、`bindings/tauri` |

## 依赖

```toml
[dependencies]
flare-im-core-sdk = { path = "../flare-im-core-sdk" }
flare-core = { path = "../flare-core" }
flare-proto = { path = "../flare-proto" }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
```

## 快速开始

```rust
use flare_im_core_sdk::{ClientConfig, FlareIMClient};
use flare_core::common::config_types::TransportProtocol;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ClientConfig::builder()
        .server_url("wss://im.example.com")
        .protocols(vec![TransportProtocol::WebSocket])
        .race_timeout(Duration::from_secs(5))
        .user_id("user_123")
        .device_id("device_456")
        .token("your_token")
        .build()?;

    let client = FlareIMClient::new(config).await?;
    client.login("user_123", "your_token").await?;
    Ok(())
}
```

字段与错误类型以源码中 `ClientConfig`、`FlareIMClient` 为准。

## 示例

| 路径 | 说明 |
|------|------|
| `examples/*.rs` | Rust 示例（`complete_client`、`two_clients_chat` 等） |
| `examples/tauri/` | Tauri 桌面示例 |
| `bindings/c/` | C FFI 构建与集成 |
| 单仓 `examples/flare-core-flutter/` | Flutter + FFI（位于 monorepo 根目录） |

```bash
cd flare-im-core-sdk
RUST_LOG=info cargo run --example complete_client
```

## 架构

```
flare-im-core-sdk/
├── src/           # 客户端主逻辑
├── storage/       # SQLite 等
├── bindings/c/    # C ABI
└── bindings/tauri/
```

`flare-core` 负责连接，`flare-proto` 负责契约，本库负责会话、消息、同步与持久化边界。

## 文档

- [bindings/c/README.md](bindings/c/README.md) — C FFI
- [docs/upload_progress_event_protocol.md](docs/upload_progress_event_protocol.md) — 上传进度事件

## 开发

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

### Features（节选）

- `extensions` — 扩展点
- `storage-tools` — 存储调试
- `lifecycle-sqlite` — SQLite 生命周期集成

## 能力插件

内置能力框架（`src/capability`），默认注册 `sdk.plugin.av`（RTC/SFU）。配置项含 `capability_url`、`tenant_id`；FFI 侧见 `bindings/c` 中 capability 相关 dispatch。

## 相关仓库

- [flare-core](../flare-core/) — 长连接（MIT）
- [flare-proto](../flare-proto/) — 协议
- [flare-im-core](../flare-im-core/) — 服务端（Apache-2.0）

## 联系

`flare1522@163.com`；Issue / MR 见仓库托管平台。
