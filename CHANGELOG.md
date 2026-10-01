# 变更记录

本文件只记录**已发布到 crates.io 的版本**。版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。

## 1.2.0 — 未发布

### 新增

- 跟进契约层新增的两个会话参与者字段（需要 `flare-proto` 2.1.0）：
  `is_mention_only`（只接收提到我的消息）与 `visible_from_seq`（可见历史下限）。
- `PresenceApi::register_push_token` / `unregister_push_token`：为本设备登记、注销离线推送令牌，
  走 Core API 网关的 `PUT` / `DELETE /api/v1/devices/{device_id}/push-token`（需要带该接口的 Core 网关）。
  用户与租户由网关从 IM 令牌确定，设备取 SDK 配置的 `device_id`；推送通道目前只支持 `getui`。
- 扩展门面补齐两项：`prelude::TlsConfig`（`HttpClient::with_tls` 与 `SdkConfig::ca_trust_tls_config`
  的参数和返回类型）与 `spi::delay`（跨平台异步延时）。业务 SDK 只允许经 `spi` / `model` / `prelude`
  调用 Core SDK，此前要用这两项只能越过门面。

### 修复

- 登出时的在线状态登出不再必然失败：网关回的是 `{"success": bool}`，SDK 却按空体解码，
  每次都记「parse http json failed: error decoding response body」再退回直接断开传输。
  `success: false`（Online 没结束会话）现在按失败处理。
- 服务端以 `TENANT_UNAVAILABLE` 踢线或拒绝建连时，SDK 发出
  `KickedOff { reason: "TENANT_UNAVAILABLE" }` 并终止会话，不再自动重连、不再为它换令牌；
  断开后的第一次重连快照也不再提前换令牌。建连失败保留服务端给的错误类别（鉴权类、
  `PERMISSION_DENIED`），不再一律降级成 `CONNECTION_FAILED`。
- QUIC 连接被踢后迟迟不重连、断开被误报成令牌被拒，根因在 flare-core 客户端，随其修复一并生效。
- 调度与恢复路径的失败改为记日志，不再静默吞掉。此前这类失败没有任何信号，
  只能靠用户报障才发现。
- `media.user_download_get_saved_path` 只返回仍在磁盘上的文件：用户删掉或挪走了保存的文件，
  它删掉这条下载记录并返回 `null`，客户端据此把「在文件夹中显示」换回「下载」。

### 依赖

- `flare-proto` / `flare-grpc-proto` → 2.1.0，`flare-core` → 1.1.1。
  后者含三批 RUSTSEC 修复，**建议一并升级**。

## 1.1.0 — 2026-08-03

与契约层 `flare-proto` 2.0.1 对齐的发布。
