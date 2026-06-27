# Flare IM Core SDK

[![Rust](https://img.shields.io/badge/rust-1.94+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

`flare-im-core-sdk` 是 Flare IM 的统一客户端核心。Rust core 负责 SDK 生命周期、消息/会话本地投影、同步任务入口、事件体系、媒体记录、能力包与扩展事件；C、Tauri、UniFFI、Wasm bindings 只做 ABI/IPC/语言边界转换。

## 核心边界

| 层 | 职责 |
| --- | --- |
| `flare-core` | 传输、连接、帧、协商、心跳等基础通信能力 |
| `flare-proto` | 唯一 wire contract，包含 `DataPacket`、`Message`、`SyncRes`、`CapabilityPacket` 等 |
| `flare-im-core-sdk` | 客户端 IM 行为、离线本地状态、投影、outbound queue、事件路由、扩展能力入口 |
| bindings | C/Tauri/UniFFI/Wasm 的边界适配，不复制核心语义 |

## 当前生产模块

`src/lib.rs` 只 re-export 当前 public contract。SDK 行为按边界放在 `client`、`application`、`domain`、`core`、`infrastructure`、`platform` 和 `extension` 中；旧 prototype route facade 不再是生产合同。

| 模块 | 职责 |
| --- | --- |
| `client/` | `IMClient` lifecycle、typed APIs、builder、登录会话、跨端 SDK 入口 |
| `application/` | 用例编排、message/sync/presence/capability adapter、projection 更新 |
| `domain/` | 消息、会话、同步游标、pending send 等业务不变量 |
| `core/` | 事件总线、dispatcher、可靠队列、sync orchestrator |
| `infrastructure/` | protobuf codec、packet sender、socket/http transport、memory/sqlite persistence |
| `platform/` | media、transport、宿主能力端口 |
| `extension/` | capability registry、middleware、RTC/SFU capability id helpers |

## 快速开始

```rust
use flare_im_core_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<()> {
    let client = IMClient::new();
    client.init(Some("example".into()), None).await?;

    let token = IMClient::generate_core_token(CoreTokenConfig {
        secret: "insecure-secret".to_string(),
        issuer: "flare-im-core".to_string(),
        user_id: "user_a".to_string(),
        ttl_secs: 3600,
        device_id: None,
        tenant_id: Some("0".to_string()),
    })?;
    let apis = client
        .login("user_a", Some(&token), LoginDbKind::Sqlite, |_, _| {})
        .await?;

    let conversation = apis
        .conversation_api
        .get_one("peer_b", &flare_im_core_sdk::model::conversation::ConversationType::Single)
        .await?;
    let message = apis
        .message_build_api
        .create_text(&conversation.conversation_id, "hello", false)
        .await?;
    let ack = apis.message_api.send_no_oss(message).await?;
    println!("sent: {:?}", ack);
    Ok(())
}
```

## Binding Runtime

Bindings 统一通过 `bindings/shared` crate 的 `BindingRequest` 进入当前 typed SDK。这个 shared runtime 只接入已经明确迁移到 vNext 的 route；旧 prototype JSON route 不会被兼容复活。

```rust
let response = flare_im_core_sdk_bindings_runtime::invoke_json(
    &client,
    r#"{"route":"sdk.state","params":{}}"#,
).await;
```

Contract 源在 `bindings/contract/*.json`；修改后运行 `rtk cargo xtask codegen` 更新生成表。`call_signal.proto` 已移除，RTC/SFU 通过 `DataPacket.capability` 和 `rtc.*` capability id 发送。

## 事件体系

事件统一使用 `SdkEvent`：

- `EventBus::subscribe()` 提供 typed Rust 事件流。
- message mutation 事件来自 `event.proto` oneof payload；typing/presence/RTC 不占用 `conversation_seq`，走 DATA realtime/capability。
- `bindings/contract/events.json` 维护跨平台 event id、C code 和 `im://*` 名称。
- 自定义业务事件走 `ExtensionEvent` 或 `MessageEvent::Custom`，payload bytes 对 core 不透明。

## Outbox 和同步

- 发送时先写入 bounded pending outbox；离线时只入队，`Ready` 且 transport 已连接时才即时发送。
- `SendAck` accepted/error 都会收敛本地消息状态并移除 pending。
- realtime 下行消息发现 `conversation_seq` 缺口时记录 gap，并通过单会话 sync 请求补拉。
- 会话列表分页 cursor 使用服务端 string cursor；本地 adapter 只在需要时解析数值水位。
- `SyncRes` / `EventEnvelope` 当前字段以 `max_conversation_seq`、`next_cursor` 和 oneof payload 为准。

## Bindings

| Binding | 目录 | 验证 |
| --- | --- | --- |
| C ABI | `bindings/c` | `cargo check -p flare-im-core-sdk-ffi --all-targets` |
| Tauri | `bindings/tauri` | `cargo check -p flare-im-core-sdk-tauri` |
| UniFFI | `bindings/uniffi` | `cargo check --manifest-path bindings/uniffi/Cargo.toml`; `cargo test --manifest-path bindings/uniffi/Cargo.toml` |
| Wasm | `bindings/wasm` | `cargo check -p flare-im-core-sdk-wasm --target wasm32-unknown-unknown` |

## 开发验证

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-features --all-targets
rtk cargo test --workspace --all-features
rtk cargo check --manifest-path bindings/uniffi/Cargo.toml --all-features --tests
rtk cargo test --manifest-path bindings/uniffi/Cargo.toml --all-features
rtk cargo check -p flare-im-core-sdk-wasm --target wasm32-unknown-unknown --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## 服务端联调

本地服务端全栈由 `../flare-im-core/scripts/start_server.sh` 启动，并可用 `../flare-im-core/scripts/check_services.sh` 检查。当前 live gateway E2E 需要按 `SocketTransport` / `PacketSender` / typed API 重建；不要恢复旧 `memory://local` route facade 测试。

```bash
rtk bash ../flare-im-core/scripts/check_services.sh
```

## 许可

Apache License 2.0。底层 `flare-core` 为 MIT，见对应仓库许可文件。
