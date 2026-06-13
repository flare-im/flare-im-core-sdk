# Flare IM Core SDK 测试说明

当前测试以生产 Rust core contract 为中心，验证 builder、typed API、事件总线、协议映射、存储投影等共享行为。

旧 prototype 时代的 JSON route facade、`TextMessageRequest`、`SubscriptionSpec`、`EventKind`、旧 `TransportPort` 测试已移除；这些不再是当前 SDK 的生产合同。Platform bindings 应通过各自 runtime/contract crate 覆盖跨语言 JSON 或 FFI 表面，不再把旧路由层塞回 core SDK。

## 本地确定性测试

```bash
rtk cargo test -p flare-im-core-sdk
rtk cargo test --workspace
```

覆盖范围：

- `core_sdk_contract_test.rs`：当前 `IMClient::builder()` 装配合同、未登录写侧拒绝语义、内存 message/conversation store 对当前 `IMMessage` / `Conversation` 模型的保存与查询。
- crate 内 `#[cfg(test)]` 单元测试：消息内容 oneof 映射、dispatcher、sync adapter、event applier、reliable queue、SQLite repository 等靠近领域和协议边界的行为。
- `bindings/shared` crate test：binding contract route discovery 覆盖生成 API ids 与 vNext runtime routes。
- `bindings/uniffi` crate test：UniFFI object facade 通过共享 JSON runtime 调用 `sdk.state`，并验证旧 snapshot/restore JSON 入口明确返回 unsupported。

## Binding 和目标平台验证

```bash
rtk cargo check -p flare-im-core-sdk --target wasm32-unknown-unknown
rtk cargo check -p flare-im-core-sdk-wasm --target wasm32-unknown-unknown
rtk cargo check -p flare-im-core-sdk-tauri
rtk cargo check -p flare-im-core-sdk-ffi --all-targets
rtk cargo check --manifest-path bindings/uniffi/Cargo.toml
rtk cargo test -p flare-im-core-sdk-bindings-runtime
rtk cargo test --manifest-path bindings/uniffi/Cargo.toml
rtk bash scripts/verify.sh
```

## 服务端验证状态

本仓当前 vNext 测试不会伪造真实服务端 E2E。服务端健康检查入口在：

```bash
rtk bash ../flare-im-core/scripts/check_services.sh
rtk cargo test -p flare-im-core-sdk --features integration-tests --test <new_live_gateway_test>
```

live gateway E2E 需要按当前 `SocketTransport` / `PacketSender` / typed API 重建，不再沿用旧 `memory://local` 和 route facade。media、presence/typing、rich-doc、capability/RTC/SFU 的 provider-backed 服务端 E2E 可在对应服务暴露稳定测试接口后继续扩展。
