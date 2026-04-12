# Flare IM Client SDK

[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](./Cargo.toml)

**一套代码，多端到达。** Flare IM Client SDK 用 Rust 把长连接、消息、会话、同步与本地存储拧成清晰边界：上层无论是 **Rust 原生**、**C FFI（Flutter / iOS / Android / 鸿蒙等）** 还是 **Tauri / Web**，都能复用同一套协议与领域模型，少写胶水、多写业务。

如果你正在搭 IM 客户端，想要 **类型安全 + 可测试 + 可渐进替换**，从这里开始会很省时间。

---

## ✨ 为什么选择它

| 亮点 | 说明 |
|------|------|
| **协议与核心对齐** | 与 `flare-core` 长连接、`flare-proto` 消息模型同源演进，减少「客户端一套、服务端一套」的漂移 |
| **存储可插拔** | SQLite（桌面 / 移动）与 IndexedDB（Web）等后端抽象一致，便于联调与测试 |
| **FFI 一等公民** | `bindings/c` 输出稳定 C ABI，Flutter 等端通过 `cdylib` / 静态库接入（见下方示例路径） |
| **事件驱动 UI** | 连接、消息、会话、同步等事件统一出口，方便 Riverpod / Vue / 任意框架绑定 |

---

## 功能概览

### 核心能力

- **跨平台**：Rust 原生、WASM（Web）、C FFI（移动 / 桌面宿主）、Tauri 示例等
- **长连接**：基于 `flare-core` 的 WebSocket / QUIC 能力（以当前 feature 与配置为准）
- **消息**：文本、富媒体、引用、反应等（随 proto 扩展）
- **同步**：基于 seq 的增量与分页拉取
- **会话**：列表、未读、置顶静音等本地视图
- **媒体**：上传 / 下载与进度回调
- **事件总线**：统一订阅 SDK 内状态变化

### 工程化

- **Rust 2024**、`rust-version` 与 workspace 对齐
- **可选 feature**（如 `extensions`、`storage-tools`）按需裁剪体积

---

## 🚀 快速开始

### 依赖（`Cargo.toml`）

```toml
[dependencies]
flare-im-core-sdk = { path = "../flare-im-core-sdk" }
flare-core = { path = "../flare-core" }
flare-proto = { path = "../flare-proto" }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
```

### 最小 Rust 示例（示意）

```rust
use flare_im_core_sdk::{FlareIMClient, ClientConfig};
use flare_core::common::config_types::TransportProtocol;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = ClientConfig::builder()
        .server_url("wss://im.example.com")
        .media_base_url("https://media.example.com")
        .protocols(vec![TransportProtocol::QUIC, TransportProtocol::WebSocket])
        .race_timeout(Duration::from_secs(5))
        .user_id("user_123")
        .device_id("device_456")
        .token("your_token")
        .build()?;

    let client = FlareIMClient::new(config).await?;
    let login_result = client.login("user_123", "your_token").await?;
    println!("登录成功: {:?}", login_result);
    Ok(())
}
```

更多字段与错误处理请参考源码中的 `ClientConfig` 与示例。

---

## 📂 示例与端上工程

| 类型 | 路径 | 说明 |
|------|------|------|
| **Flutter（推荐）** | 单仓 `examples/flare-core-flutter/` | 完整 Flutter 应用，集成 C FFI、`macOS/iOS` 构建脚本与产物拷贝说明 |
| **Rust 示例** | 本仓库 `examples/*.rs` | `complete_client`、`two_clients_chat`、E2E 等 |
| **Tauri** | `examples/tauri/` | Web 技术栈桌面示例 |
| **C FFI 构建** | `bindings/c/` | `Makefile`、`cargo build -p flare-im-core-sdk-ffi` |

> Flutter 示例已从本仓库子路径 **迁至单仓根目录** `examples/flare-core-flutter/`，与 `flare-im-core-sdk` 并列；请在 monorepo 根下进入该目录执行 `flutter run`。

### 运行 Rust 示例

```bash
cd flare-im-core-sdk

RUST_LOG=info cargo run --example complete_client

RUST_LOG=info cargo run --example two_clients_chat

RUST_LOG=info SERVER_URL=ws://localhost:60051/ws USER_ID=user123 \
  cargo run --example complete_client
```

---

## 🏗️ 架构速览

```
flare-im-core-sdk/
├── src/                 # 客户端主逻辑、配置、服务门面
├── connection/          # 连接（flare-core）
├── storage/             # SQLite / IndexedDB 等
├── bindings/c/          # C ABI（Flutter / 原生宿主）
├── bindings/tauri/      # Tauri 侧封装
└── examples/            # Rust 与 Tauri 等示例
```

设计要点：**flare-core 管连接、flare-proto 管契约、本库管会话 / 消息 / 同步与持久化边界**。

---

## 📚 文档

- [REFACTOR_ARCHITECTURE.md](./REFACTOR_ARCHITECTURE.md) — 架构说明  
- [REFACTOR_PLAN.md](./REFACTOR_PLAN.md) — 演进计划  
- [docs/upload_progress_event_protocol.md](./docs/upload_progress_event_protocol.md) — 上传进度事件约定  
- [bindings/c/README.md](./bindings/c/README.md) — C FFI 使用说明  

---

## 🔧 开发

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

### Cargo features（节选）

```toml
flare-im-core-sdk = { path = "../flare-im-core-sdk", features = ["extensions"] }
```

- `extensions` — 扩展点  
- `storage-tools` — 存储调试工具  
- `lifecycle-sqlite` — 与 SQLite 生命周期相关的集成能力（见 `Cargo.toml`）  

---

## 📦 相关仓库（同 monorepo）

- [flare-core](../flare-core/) — 长连接与传输  
- [flare-proto](../flare-proto/) — 协议与消息结构  
- [flare-im-core](../flare-im-core/) — 服务端核心  

---

## 📬 联系与交流

想交流集成方案、反馈 Bug、或聊 IM 客户端架构，欢迎发邮件：

**flare1522@163.com**

（若使用企业内部 Git，也可通过仓库 Issue / MR 流程联系维护者。）

---

## 📄 License

MIT — 见根目录 `Cargo.toml` 中 `license = "MIT"`；若需单独分发许可证正文可自行补充 `LICENSE` 文件。

---

**Flare IM Client SDK** — 把复杂留给自己，把简单留给你的产品界面。
