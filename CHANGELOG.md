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
- 服务端暂时放不了行时（网关读不到租户状态等，flare-core 1.1.1 以 `SERVICE_UNAVAILABLE`
  拒绝建连），建连错误保留 `SERVICE_UNAVAILABLE`，不再降级成 `CONNECTION_FAILED`；
  会话中的重连循环把它当可重试，照常退避重连，不换令牌、不终止会话。新增回环集成测试
  `tests/transient_rejection_reconnect.rs`：本机起 flare-core 服务端，驱动 SDK 走完整的
  断线、被暂时性拒绝、退避、再连上的循环，并以 `TENANT_UNAVAILABLE` 终止会话作对照。
- QUIC 连接被踢后迟迟不重连、断开被误报成令牌被拒，根因在 flare-core 客户端，随其修复一并生效。
- 发送被服务端暂时拒绝时不再判失败。SendAck 错误码为 `UNAVAILABLE`、`TIMEOUT`、`RATE_LIMITED`、
  `SLOW_DOWN`、`NEEDS_RETRY` 时（例如 message-ingest 读不到租户投影，或同一条消息首发仍在处理），
  可靠队列让消息保持发送中，退避后用同一个 `client_msg_id` 重发；ingest 按它做摄入幂等，不会多出
  一条消息。退避从 1 s 起翻倍，封顶 30 s，带 ±20% 抖动。重发与发送失败共用 `ack_max_retries`
  的次数预算，用完后按服务端给的原因判失败。其余错误（含 `TENANT_UNAVAILABLE`）仍立即判失败。
  重发期间应用收不到错误 SendAck。新增回环集成测试 `tests/transient_send_rejection_retry.rs`。
- 经 `IMClientBuilder::config` 设置的 `reconnect_interval_secs` 与 `max_reconnect_attempts` 现在也作用于
  SDK 的重连循环。此前这两项只传到 flare-core 客户端，SDK 自己的循环仍按 `init()` 的覆盖项或默认
  5 s、不限次数退避。两处都设置时，`init()` 的覆盖项优先。
- 会话结束时（登出、断开、重新登录、终止会话、销毁客户端），该会话的重连监视器立即停止。此前它会
  睡满当前退避，醒来后在已结束的会话上记一条「SDK reconnect succeeded」。
- 服务端停机（flare-core 下发 CLOSE(`SERVER_SHUTDOWN`)，或 WebSocket 1001、QUIC 应用关闭带来同一
  原因）后，重连循环的第一次尝试在 1–5 s 之间随机等一段，之后照常按退避（封顶 30 s、±20% 抖动）。
  此前停机后按错误处理：发送前的惰性重连立即重试，撞上还没起来的实例，所有客户端同一刻涌向刚起来的
  实例。新增回环集成测试 `tests/server_shutdown_reconnect.rs`。
- 会话结束时，状态快照、终局事件、令牌刷新三个监视器也立即停止。此前令牌刷新监视器要睡到下一次
  刷新（可能几十分钟后）、另两个要等到下一条连接事件，才发现会话已结束。
- 会话在一次重连期间（或开始之前）结束时，这次重连返回明确的错误（`NOT_CONNECTED`，
  「session ended before the reconnect completed」），不再返回成功；网络切换触发的主动重连
  （`notify_network_change`）因此不会向应用报告一次不存在的成功。
- 重连被拒时按错误码（`PERMISSION_DENIED`）识别租户不可用，不再在错误文本里找
  `TENANT_UNAVAILABLE`；断开原因按类型化原因整体比对。
- 重连退避的抖动按毫秒取值。退避本身不变（从配置的间隔起翻倍、封顶 30 s、±20%），但此前按整秒
  取值：5 s 间隔的第一次重连只落在 4、5、6 s 三个点上，服务端重启或暂时拒绝（`SERVICE_UNAVAILABLE`）
  后同一批断开的客户端被分成三拨同时涌上来。
- 调度与恢复路径的失败改为记日志，不再静默吞掉。此前这类失败没有任何信号，
  只能靠用户报障才发现。
- 长连接断开后只有一条重连路径：会话的重连监视器。此前 flare-core 客户端还会在断开后的下一次发送时
  自己重连，与监视器新建的客户端并行：旧客户端带着建连时的令牌连上，监视器又新建一个，同一用户可能
  出现两条连接（网关按设备冲突处理），旧客户端连上后也没有重连后的同步。现在 SDK 建连时关掉 flare-core
  的发送前重连（`with_reconnect_on_send(false)`，需要含它的 flare-core），断开后不经可靠队列的发送
  （撤回、已读回执、输入状态、同步请求等）直接返回连接错误。
- 连接以错误终止时（flare-core 的连接错误是终止事件，例如读到非法帧、读写出错），SDK 除了原有的
  `ServerError` 还发一次 `Disconnected { reason: "transport error: …" }`，重连监视器据此重连。此前
  只有 `ServerError`，监视器不动，状态停在 Ready、连接却已经死了，要等下一次发送把旧客户端拉起来。
- 可靠队列：一次发送尝试没能发出去（未连接、写失败、发送超时）的消息，在长连接恢复时立即重发，
  不再等到发送超时（`ack_timeout_secs`，默认 10 s）。重试次数照旧累计。
- 不经可靠队列的直发路径（存储不带 PendingSend 仓储时）没有暂时性重试，已在
  `SendMessageCommand::execute` 与 `src/infrastructure/README.md` 注明：生产登录路径（SQLite、加密
  SQLite、Web 的 IndexedDB）都带 PendingSend 仓储、走可靠队列，只有 `in_memory_empty_im_provider`、
  运行时 `StorageKind::Memory` 与 `FlareAppClientBuilder` 在登录前的默认存储会走它。
- 新增回环集成测试 `tests/single_reconnect_path.rs`：经本机 TCP 中继切断连接或注入非法帧，断言断线
  期间的发送不建连、只由监视器在退避后重连一次、全程最多一条连接，断线期间发出的消息在重连后立即被
  接收。服务端停机用例（`tests/server_shutdown_reconnect.rs`）现在也能单独验证监视器的停机暂停：
  此前旧客户端的惰性重连会先把连接恢复。
- `media.user_download_get_saved_path` 只返回仍在磁盘上的文件：用户删掉或挪走了保存的文件，
  它删掉这条下载记录并返回 `null`，客户端据此把「在文件夹中显示」换回「下载」。

### 依赖

- `flare-proto` / `flare-grpc-proto` → 2.1.0，`flare-core` → 1.1.1。
  后者含三批 RUSTSEC 修复，**建议一并升级**。

## 1.1.0 — 2026-08-03

与契约层 `flare-proto` 2.0.1 对齐的发布。
