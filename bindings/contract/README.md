# Flare IM Bindings Contract

**唯一配置目录**：日常扩展 bindings 只改本目录下的 JSON，然后执行：

```bash
make -C bindings codegen
make -C bindings codegen-check
```

`contract/tools/*.py` 是 **codegen 执行器**（类似 protoc），不是业务配置源。新增 API / 事件时 **不要改 Python**。

## 配置文件

| 文件 | 用途 |
|------|------|
| `manifest.json` | 契约版本、平台映射、产物路径 |
| `apis.json` |  canonical API id、C 符号、Tauri 命令名、core 方法 |
| `dispatch.json` | JSON dispatch 操作表（message / conversation / media / capability / message_build） |
| `direct_invoke.json` | `IMClient` 直调路由（sync / presence / connection / diagnostics） |
| `c_typed_abi.json` | 保留稳定 C 符号的 typed shim（参数解析 → `invoke_api_id`） |
| `client_config.json` | init / `SdkConfigOverlay` 字段说明与示例（含传输与协议竞速） |
| `events.json` | 事件 id、C code、`im://*` 名称、C JSON 形状说明 |
| `errors.json` | 稳定错误码 |

## 初始化与传输配置

所有平台的 init 使用同一 JSON 形状（`snake_case`）：

- 裸 [`SdkConfigOverlay`](../../src/client/lifecycle.rs)（C `flare_sdk_init` 传统用法）
- 或 `{ "environment": "...", "sdk_config": { ... } }`（推荐，与 `client.init` / `sdk.init` 一致）

关键字段：

| 字段 | 说明 |
|------|------|
| `transport_policy` | `auto` / `websocket_only` / `protocol_race`（WASM 运行时强制 `websocket_only`） |
| `default_transport` | `websocket` / `quic`，非竞速时首选 |
| `protocol_race_order` | 如 `["quic","websocket"]`，竞速优先级（前项优先） |
| `ws_url` / `quic_url` | 竞速需同时配置 |

示例见 `client_config.json`；codegen 产出 `CLIENT_INIT_REQUEST_EXAMPLE_JSON` 与各平台 `flare_client_*` / `FLARE_SDK_CONFIG_EXAMPLE_JSON`。

统一 invoke：`client.init` → `sdk.init`（`direct_invoke.json`），Tauri 仍保留 `sdk_init` 命令透传同一结构。

## 新增能力 checklist

### JSON 类 API（`sdk_invoke` / `flare_*_dispatch_json`）

1. 在 `apis.json` 增加 method（`id`、`core`、`c`、`tauri` 等）
2. 若走 dispatch：在 `dispatch.json` 对应 `group.operations` 增加 `op` 定义
3. `make -C bindings codegen`

### IMClient 直调（sync / presence / 连接态等）

1. 在 `apis.json` 增加 method
2. 在 `direct_invoke.json` 的 `routes` 增加一条（`route` 与 `operation.normalize` 后的名字一致）
3. `make -C bindings codegen`

### 媒体上传/下载（`flare_media_*`）

1. 在 `apis.json` 增加 method
2. 路径类上传 / 删文件：在 `dispatch.json` → `media.operations` 增加 `op`（可用 `optional_upload_options`、`bytes_vec`）
3. 下载到用户目录 / 同步取消：在 `direct_invoke.json` 增加 `route`（native 用 `cfg: not(target_arch = "wasm32")`）
4. 稳定 C 符号：在 `c_typed_abi.json` 增加 export（`upload_options` / `bytes_view` / `request_json` / `sync_bool_invoke`）
5. `make -C bindings codegen`

其余媒体控制（URL、缓存、子目录等）继续走 `flare_media_dispatch_json`。

### 稳定 C 符号（`flare_sdk_sync_*` / `flare_message_*` 等）

1. 在 `apis.json` 增加 method（含 `c` 符号名）
2. 在 `c_typed_abi.json` 的 `exports` 增加一条：`symbol`、`api_id`（与 apis id 一致）、`kind`（`invoke_unit` / `invoke_json` / `invoke_send_ack`）、`args`
3. 若 `api_id` 尚未在 `direct_invoke.json` 或 `dispatch.json` 中实现，先补对应路由
4. `make -C bindings codegen` → 生成 `c/src/generated/typed_abi.rs`

### 事件

1. 在 `events.json` 的 `events` 数组增加一条（`id`、`cCode`、`cCodeName`、`tauri`、`cJson`）
2. 在 core 增加对应 `SdkEvent` 变体（Rust 侧仍是真相源）
3. `make -C bindings codegen` — 自动生成：
   - `runtime/.../event_registry.rs`
   - `tauri/.../event_emit.rs`（转发入口）
   - `wasm/.../events.rs`、`uniffi/.../events.rs`（契约表）
4. 若 payload 形状全新：在 `runtime/src/event_bridge.rs` 补 **一次** C JSON 序列化；Tauri 侧在 `convert.rs` 补强类型 payload（可选）。`c/src/event.rs` 仅保留订阅/转发

## 平台 crate 职责（保持瘦）

| 平台 | 手写保留 | 自动生成 |
|------|----------|----------|
| **c** | 句柄、回调、内存、上传二进制、事件订阅 | `typed_abi`、`json_dispatch`、`invoke`、`events` |
| **tauri** | `lifecycle`（AppHandle）、`convert`（SdkEvent→payload） | `handler`、`invoke`、`event_emit` |
| **wasm** | smoke runtime、宿主 port | `contract`、`bindings`、`events` |
| **uniffi** | 后续 session adapter | `contract`、`types`、`events`、`invoke` 占位 |

## 统一调用入口

- **C**：`flare_sdk_invoke_json(api_id, params_json, …)` + 生成的 legacy dispatch 符号
- **Tauri**：`sdk_invoke(api_id, request)` + `sdk_init` / `sdk_login` / `sdk_logout`
- **Wasm（smoke）**：`flareInvoke(runtime, api_id, request_json)`

契约 `api_id` 与 `apis.json` 中 `id` 字段一致（如 `messages.list`、`sync.conversation`）。
