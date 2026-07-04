# 同步收敛重设计：本地优先三层收敛 + 设备全局同步点

## Goal

在 `flare-im-core-sdk` 内，把冷启动 / 热启动 / 断线重连的数据同步与补全重构为**注意力驱动的本地优先三层收敛**，并引入**设备级全局同步点**。可检查的完成定义：

1. **热启首屏 <200ms**：会话列表与上次活跃会话时间线从本地纯读出图，不 await 传输（T0 本地水合）。
2. **冷启可略慢但有界**：本地空时只 gate「首屏 top-N 会话摘要 + 前台会话消息」，其余静默回填；主线程工作 <500ms，UI 出骨架。
3. **重连不阻塞**：`reconnect()` 不再 `stop_sync + 全量重同步`；走 `CatchingUp` 静默态 + 全局游标有界流式 catch-up、前台优先；网络切换/前台回归的高频触发下依然轻量。
4. **catch-up O(增量) 而非 O(会话数)**：一次 `SyncSince(globalSeq)` 流式请求替代逐会话 N 次同步（消灭 catch-up N+1），已读上报同样从 O(会话数) 降为增量。
5. **收敛按注意力排序**：前台会话 > 可见列表窗口 > 近期活跃 > 其余；事件驱动优先级调度器取代扁平轮询 + 周期前台轮询；媒体缩略图/blurhash 随注意力预热。
6. **多端收敛正确**：已读位点、reaction、edit、recall、pin/mark、retention 跨端 LWW 收敛；乱序/缺目标事件可重放；均沿用现有 `SyncEventApplier` 语义，不重造。
7. **领域无关的收敛内核**：core 是通用同步内核 + `SyncDomain` SPI；消息/会话只是内置头两个领域。群、好友、任意业务对象作为新 `SyncDomain` 注册即白拿冷启/热启/重连/追赶/优先级/缺口修复。内核只提供**机制**；群/好友的**业务语义**留在 Social/插件注册进来（守 spec 4/5）。
8. **可扩展**：`channels/threads/收藏/多端草稿/presence 汇总/pin/reaction 聚合` 及第三方业务系统零改引擎接入，声明领域 lane 即被调度与多路复用追赶。
9. **广播/频道读扩散**：大群/频道走 fan-out-on-read 共享时间线（客户端只订阅+虚拟化消费，不物化百万成员），小群保持 fan-out-on-write；百万级读扩散主体在服务端，SDK 消费共享时间线秒开+流畅。
10. **完整媒体管线**：渐进呈现（占位→blurhash→低清→全清）+ 上/下行断点续传 + 内容寻址有界缓存 + 全程脱离渲染帧；转码/CDN/签名 URL 为服务端+基础设施里程碑。

## Constraints & decisions

- **服务端可改（已确认）**：主设计直接按 `SyncSince(globalSeq)` 全局增量流；服务端 endpoint 作为并行里程碑（跨仓 flare-im-core）。保留「diff 出变化会话再逐会话 catch-up」作服务端未就绪期降级。
- **本次已进入执行**：本仓可自动落地的 core-sdk / packages / web 验证项持续推进；跨仓服务端、基础设施、Social 参照领域、三端人工场景按执行协议标 `blocked(cross-repo/env)` 或 partial。
- **保留并复用的现有强资产**（不得重造）：
  - `SyncEventApplier`（`usecases/sync/event_applier.rs`）——recall/edit/reaction/delete/read/retention/pin/mark/custom/conversation 全事件的 `operation_seq` **LWW 冲突收敛** + `missing_target_is_already_covered` **乱序/缺目标可重放**语义。全局流应用器必须复用它。
  - `IncomingMessageConverger`——身份规范化(单聊 canonical id) + pending 合并回 ack(`MergePendingAndAck`) + 服务端重复消息去重/状态刷新。多端 echo 收敛已在此。
  - `seq_repair.rs` 实时缺口退避补拉；有界串行持久化 worker。
  - 现有扩展 seam：`src/spi/mod.rs`（稳定导出 `SyncTask`/能力插件/扩展注册/`ContentCodec`/`MessageInterceptor`/`EventInterceptor`）、`SdkCapabilityRegistry`（capability_id 前缀派发）、`IMClientBuilder::add_sync_task`。
- **领域无关内核（本轮新定位）**：core = 通用收敛内核 + `SyncDomain` SPI；messages/conversations 为内置领域，群/好友/任意业务作为新领域注册。**机制在 core、语义在 Social/插件**。为此需打开三处硬耦合：
  1. **applier 硬编码** → applier 注册表按 `domain_id` 路由（`SyncEventApplier` 降为 `im.messages` 内置领域的 applier，不再是唯一入口）。
  2. **游标消息形** `SyncCursorVo{conversation_id,last_seq}` → 抽象 per-domain 子游标，挂设备全局同步点下（好友列表无 conversation_id）。
  3. **Scope 闭枚举** `SyncScope` → 开放 `DomainId`(稳定字符串)。
  - 内置领域仅 `im.messages`/`im.conversations`；`social.friends`/`social.groups` 由 Social `impl SyncDomain` 注册，core 不认识其业务规则。
- **广播/媒体的分层纪律（本轮扩范围，防止污染 core）**：
  - 频道 fan-out：**读扩散主体在服务端**（flare-im-core，memory 记读扩散已确定性丢0）。core 侧只做「fan-out 模式决策(读/写阈值) + 共享时间线消费 + 订阅/退订 + 成员聚合(不枚举)」；examples 虚拟化渲染。频道是 `im.messages/conversations` 领域的一个 **sync-strategy 属性**（按 `ConversationType::{Channel,Broadcast}`），非新领域。
  - 媒体管线：**core**=缓存驱逐策略 + 上/下行续传状态机 + 渐进状态模型；**packages**=平台传输/解码/缩略图适配器；**examples**=渐进态渲染；**服务端+基础设施**=转码、CDN、签名 URL（跨仓/跨栈里程碑）。
- **flare-im-spec 硬约束**：
  - 无兼容冗余（pre-release）——一次性替换旧 `MessagesSyncTask` 扁平轮询 + 周期前台轮询 + 启动 history backfill 独立例程，**同 change 删旧路径**。
  - 分层：所有同步行为产品中立 → 沉 `flare-im-core-sdk`(Rust)；注意力信号 API + 就绪/态事件按 core→bindings(L1)→sdk-spec→packages 顺序；examples 只上报视口 + 渲染态。
  - 预算：热启首屏 <200ms；发送回显 <16ms；冷启主线程 <500ms；重连不阻塞全量重取；catch-up 中 60fps；有界内存。
  - 优先级：正确性 > 流畅 > 性能 > 其它。
- **重连是高频事件**：`notify_network_change`（wifi/蜂窝切换即重连，带 coalescing）+ 进入前台 `set_heartbeat_app_state(Foreground)` 都会触发收敛 → 轻量路径是硬要求，不是优化项。
- **注意力信号是 hint，非正确性关键**：视口错了只影响顺序，缺口修复保证最终完整。
- **排序**：显示序=服务端 `conversationSeq`；乐观本地项占位、ack 校正（保留现状）。

## 现状锚点（memory-less 未来会话必读）

- 编排：`kernel/sync/orchestrator.rs` — Init 并行 → `SyncFinished(Init)` → Background 静默并行。
- 触发：`runtime/engine.rs:264` `connect()`→`initial_login()`（**未看本地是否有缓存**）；`engine.rs:273-296` `reconnect()`=`stop_sync + transport.disconnect + initial-login 形态全量同步`（**过重**，但**未拆可靠队列**）；`engine.rs:218` connect 时 `queue.recover_pending_for_current_user()`；`engine.rs:320` `bootstrap()`→`sync_manager.run_with_context`。
- 触发（客户端）：`session_lifecycle.rs:45-53` 进前台→`sync_foreground_convergence_silent`（摘要级）；`session_lifecycle.rs:78-115` `notify_network_change`→`reconnect_current_engine`（coalesced）。
- 任务集：`application/sync_task/{conversations(Init,w10),messages(Background,w20),read_states(Background,w5),conversation_settings,key_events}.rs`。
  - **read_states.rs 缺口**：`ctx.store.conversations.list()` 后 `push_local_read_states(&list)` 对**全部会话**上报 ack → O(会话数)，每次 sync 都跑。
  - **conversations.rs**：Init 拉全量会话列表（应改 top-N 首屏 gate）。
- 游标：每会话 `SyncCursorVo{last_seq,synced_at}`；会话列表 `CONVERSATION_CURSOR_KEY`；每任务 checkpoint（`kernel/sync/checkpoint.rs`，key=`sync:checkpoint:{task_id}`，String cursor，None 不覆盖）。选择策略 `domain/sync/policy.rs::select_conversation_cursor_ms`。**无设备级全局游标**。
- 冲突/重放：`event_applier.rs` `operation_seq()` LWW；`missing_target_is_already_covered()` 用 cursor/`get_local_max_seq`/`cleared_floor` 判定目标是否已覆盖。
- 历史回补：`sync_api.rs` `backfill_conversation_history` + `request_message_backfill_before_seq`；启动 history backfill 有界（`history_backfill_limit`/`max_pages_per_conversation`/`max_conversations`）——**独立启动例程，与同步抢带宽，应并入调度器**。
- 前台收敛（**已取代周期轮询**）：`session_watchers.rs::spawn_foreground_sync_worker` 已删除；前台/可见/近期优先通过 `AttentionRegistry` + `MessagesSyncTask` 调度生效。`sync_protocol_adapter.rs` 的摘要级 foreground convergence 保留为显式 API，不再由周期 worker 触发。
- 媒体：`domain/media_cache.rs` **仅按需**，无 prefetch/warm/blurhash 预热。已有底子：`domain/upload_manifest.rs`(`MediaUploadManifestVo`/`MediaUploadPartVo{part_number,offset}`/`UploadManifestState`/`DirectUploadTransportKindVo`)=**上行断点续传清单**；`MediaCacheEntryVo`+`MediaCacheStatsVo{max_bytes}`=**有界内容缓存**；`user_file_download_store` 存在（下行续传待补）。缺：下行续传、渐进呈现(blurhash/低清)、转码/CDN(服务端)。
- 频道：`model/conversation.rs` 已有 `ConversationType::{Channel,Broadcast}`(type 8,wire "broadcast",`generate_broadcast_conversation_id`)。缺：fan-out-on-read 共享时间线同步语义 + 订阅模型 + 服务端读扩散消费。
- FSM：`kernel/fsm/sync_state.rs` Idle/Syncing/CatchingUp/Error（复用 CatchingUp 作重连可见态）。
- 任务抽象：`kernel/sync/task.rs`；`IMClientBuilder::add_sync_task` 已可注册。

## 全面分析：八维度补充（第二轮深挖结论）

1. **多端 echo 与冲突收敛**：已相当完整——converger 处理本机 pending 合并 + 服务端重复；applier 用 operation_seq LWW。**结论**：复用，不动；全局流只需把跨会话事件喂进同一 applier。
2. **已读位点收敛**：入向已读 `mark_outgoing_read_upto_seq` 已 LWW；但**本地已读上报是全量 O(N)**（read_states.rs）。**改法**：只上报"本地已读位点 > 已 ack 位点"的会话（delta），并入 P2/背景增量。
3. **未读数正确性**：`recompute_unread_for_user` 存在，由 seq 驱动。**结论**：确保 catch-up 后触发重算；纳入 D3 验证。
4. **pending 消息跨重启/重连**：connect 已 `recover_pending_for_current_user`；重连不拆队列。**结论**：验证重连不重复 recover、不丢 pending，纳入 A3/D3，非新建。
5. **历史回补与同步交叠**：backfill 已有界，但独立启动例程与 T1/T2 抢带宽。**改法**：history backfill 作为 P3 lane 并入调度器，受同一背压/并发预算约束（C2 删旧启动例程）。
6. **媒体补全**：无预热。**改法**：注意力域内为前台会话最近消息预热缩略图/blurhash（P1/P2 媒体 lane，脱离渲染、bounded、内容寻址去重），progressive 占位→低清→全清。
7. **多端草稿/设置**：draft 走 `conversation_settings` patch。**结论**：作为未来 SyncTask lane（D1 分类法覆盖），本期不实现。
8. **可观测性/预算测量**：`SyncRunContext{run_id,trigger,scope,visibility,reason}` + progress 事件已具雏形。**改法**：补每层(T0/T1/T2)时序 + catch-up 条数/请求数指标，供 D3 断言预算（首屏<200ms、冷启主线程<500ms、catch-up O(增量)）。
9. **领域无关接入（新定位核心）**：现有 `SyncTask`+`add_sync_task` 可注册自定义任务，但 applier/游标/scope 三处消息形硬耦合，好友/群无法复用追赶/缺口/多路复用。**改法**：抽出 `SyncDomain` SPI + `DomainStreamRouter`（全局流多路复用），messages/conversations 降为内置领域；`social.friends`/`social.groups` 作参照领域在 Social 实现并注册，验证"任意业务黑盒接入"。见 Phase E。

## Status: 本仓+跨仓可自动落地面全部收口（Phase A-H 前波 + I/J/K/L/M/N 优化战役 ✅，2026-07-02）
Current focus: 无在途。剩余项均为跨仓协调/真实环境类：F4 读扩散规模化（flare-im-core 生产）、G6 转码-CDN（infra）、E6 Social 参照领域（flare-social）、H1-H8 三端人工验收（env）、心跳捎带水位（flare-core transport 设计决策）、E4/B2 全局流 runtime 收编（依赖未来 SyncSince endpoint 决策，方案 B 下非必需）。
优化战役终账：sdk **461→476 passed**；今日修复 **4 个隐藏正确性缺口**（事件失败被游标越过 I1 / 列表增量双侧格式断裂 I6 / 水位与快照游标被钳制静默丢弃 L0 / 清空历史后游标卡死 M1）+ 新增 10 个协议字段/消息（bundle、方向化回溯、oneof 投递、bootstrap 上限等）；执行明细见 Phase I-N 与 Execution log。
先前状态：**core-sdk 可离线闭环的同步收敛 / 领域路由 / 媒体状态机 / 频道策略 / 热启动 NonBlocking 校准 / 冷热启动等待报告已落地并验证**。当前验证：`cargo test --features storage-sqlite,lifecycle-sqlite` → **461 passed, 5 ignored**；bindings shared **39 passed**；core-codegen/codegen-check 通过；TypeScript SDK **76 passed** + `tsc` 通过；Vue IM UI **109 passed** + `vue-tsc` 通过；web app **27 passed** + build/bundle budget 通过；tauri renderer typecheck/test **6 passed**；WASM 已重建并同步中央 artifact 到 Electron 副本且 hash 一致。剩余真实边界：(1) B3 的服务端 `MultiConversationSync` handler 已存在且客户端 batch 接线已完成，进一步 `SyncSessionHints` 填充仍属服务端可选增强；(2) F4 读扩散 / G6 转码-CDN 属 flare-im-core + infra 的生产能力扩展；(3) E6 Social 参照领域属 flare-social；(4) H1-H8 三端弱网/多账号/原生窗口人工场景依赖后端和设备环境，当前自动化覆盖到 web E2E messaging flow 与 web/tauri build/test。

## 执行协议（autonomous-ready）

用 executing-from-planning / autonomous-executor 驱动。循环：读 PLAN → 取下一个未完成且未阻塞步骤 → 实现 → **跑该步 verify** → 绿则打勾并记结果 → 否则 systematic-debugging 修到绿 → 下一步。

- **Gate 规则**：每个 Phase 末有 verify gate（见各 Phase 末 `[verify]`）。**红灯不推进下一 Phase**，也不打勾。
- **契约传播顺序（不可跳）**：core 行为 → `bindings`(L1，用 xtask 生成，勿手改) → `sdk-spec` → 生成 `packages` → examples。改了 core 契约必须按序重生成再验。
- **一次一路径（spec 1）**：替换即删旧路径，同 change 完成，不留双跑。
- **停机条件（hon3实）**：遇到需要产品决策、服务端/基础设施协作(F4/G6/E6)、真实三端环境、或 verify 连续失败达阈值 → 停并上报，不臆造。跨仓/环境里程碑步骤不可执行时标 `blocked(...)`，不阻塞其余。
- **每步落结果**：打勾时追加一行"产出/命令/数字"，供恢复会话核对边界。
- **设计红线（开发阶段·无兼容·每次改动自检，违反即回改）**：
  1. **无兼容包袱**：未上线 → 没有向后兼容义务。禁止旧+新并存、legacy 别名、`user_id/userId` 双读、`metadata` stringly 语义、prototype fallback、compat shim。**旧路径同 change 删除**，只留一条干净路径。
  2. **简洁**：能删的先删，不为"未来可能"加抽象/trait/间接层；抽象只在买到正确性或流畅性时才引入；命名与周边一致，不发明术语。
  3. **性能优先**：热路径 P50<1ms/P99<5ms(排除 I/O)；零无谓分配/克隆、批量优于 N+1、有界通道+背压、缓存有界可驱逐；性能与优雅冲突时性能赢（正确性/流畅性仍在其上）。
  4. **不阻塞交互线程**：磁盘/网络/编解码/大 JSON/无界循环一律移出渲染线程，结果经事件回投；乐观 UI 先行、异步 reconcile。
  5. **最优而非最省事**：选内核级最优解（如全局同步点多路复用、领域无关 SPI），不为赶工退化成 per-platform / per-domain 复制。

## 前置条件与环境

- **工具链**：Rust(本仓，`cargo`，feature `storage-sqlite`)；Node/yarn(web/tauri 与 packages)；Flutter(`^3.10.1`，flutter-app)；Tauri CLI。bindings 重生成用 `xtask`。（本仓是 Rust，不需 JDK；服务端 im-main 才需 JDK17。）
- **后端**：flare-im-core / im-main 服务端须在 `.env*` 指向的地址可用（ws url 见各 app `.env`/`.env.development`/`.env.local`）。E2E 前确认后端 up + 账号可登录。
- **测试账号**：**11 ~ 20 共 10 个**（user_id 字符串 "11".."20"）。登录令牌走 SDK 侧 `util_generate_core_token`/`CoreTokenConfig`（执行时确认各 app 登录入口是填 user_id 还是 user_id+token）。
- **App 启动**：web=`npm run dev`(vite:5173)；tauri=`yarn tauri dev`(renderer:1432+原生窗)；flutter=`flutter run -d macos`(或指定 device)。
- **测试工具路由**（按 MCP 分层纪律）：
  - **web-app** → 优先 **claude-in-chrome** MCP（浏览器 tier，DOM 感知，最快）；亦可复用其自带 Playwright(`npm run test:e2e`)。
  - **tauri-app** → **computer-use**（原生桌面，full tier，需 `request_access`）。
  - **flutter-app** → **computer-use**（原生桌面 macos）；或 flutter `integration_test`。
  - 多账号并发需多实例：web 多浏览器 profile/tab；tauri/flutter 多窗口或多设备。

## 验证命令板（各步/Phase 引用）

- core 单测：`cargo test`（本仓根）；热路径预算断言在 D4/F5/G7。
- bindings 重生成：`cargo run -p xtask -- <gen 子命令>`（执行时确认子命令），生成后 `cargo test` + 契约 golden。
- packages 构建：`flare-core-typescript-sdk` / `flare-core-vue-im-ui` build；web `npm run build`+`npm test`(含 verify:architecture 反 shadow-view)。
- web e2e：`npm run test:e2e`（Playwright）或 chrome MCP 脚本。
- tauri：`yarn tauri dev` 起窗后 computer-use；`yarn test`。
- flutter：`flutter test` + `flutter run`；computer-use 驱动 UI。

## Steps

### Phase A — core 状态机与就绪分层（无服务端依赖，先落地，体感提升最大）
- [x] **A1 启动分类器** — `kernel/sync/task.rs` 加 `StartupClass{Cold,Warm}`；`engine.rs::connect()` 用 `has_local_user_data()`(会话非空)分类选上下文，删「永远 initial_login」。热启动现在走 `WarmStartupCalibration` + `SyncVisibility::NonBlocking`，不阻塞首屏但会发同步状态事件。verify: `cargo test --features storage-sqlite,lifecycle-sqlite` → **461 passed, 5 ignored**。
- [~] A2 就绪三层落引擎 —
  - [x] T0 本地水合信号：`bootstrap()` 在 sync 启动前发 `Readiness::LocalReady`（本地缓存即可出图，UI 收起骨架不等网络）。397 passed。
  - [x] T1/T2 信号映射：orchestrator Init/Background `Finished` 分别加发 `ForegroundFresh`/`Converged`。
  - [x] T1 首屏 gate 有界化 — 由 I6 冷启 bundle 解决：`SyncSnapshotSync{newest_first, conversation_page_limit=50, messages_per_conversation=30}` 首页=top-N 摘要+首页消息一次 RPC，其余页静默续拉；配合 Phase N 的 bootstrap cap 放开覆盖大账号。
  - [x] T2 环境收敛：其余进优先级调度器（Phase C/B4 已接真实消息同步路径）。
- [~] A3 重连轻量化 — **connected-transport 路径已落地**：ready + same user + transport alive 时走 `CatchUpOnly`，不 `stop_sync`、不 disconnect、不重复 pending recover，并通过 `SyncManager::run_nonblocking_with_context()` 发起非阻塞追赶；真实断线仍走 transport reconnect。**剩余**：真实 `SyncSince(globalSeq)` endpoint 完成后把 catch-up 从 batch 降级路径替换为全局游标流。verify: `reconnect_plan_uses_catch_up_only_when_ready_transport_is_alive`、`nonblocking_catch_up_does_not_abort_current_sync_run`。
- [x] A4 就绪/态事件 — `ReadinessStage{LocalReady,ForegroundFresh,Converged,Degraded}` + `SyncNotify::Readiness{run,stage}`；`SyncNotify::should_publish()` 区分 silent drop 与 nonblocking status emit，热启动校准会发 `sync.started/progress/finished/state/readiness`，但不触发 blocking loading。bindings/sdk-spec/packages 已传播 `sync.readiness` 与 run context 字段。verify: bindings shared **39 passed**、codegen-check 通过。

### Phase B — 设备全局同步点（跨仓，含降级）
- [x] **B1 core 侧全局游标** — `DeviceGlobalSyncPoint` + `DeviceGlobalSyncPointStore` 复用 raw cursor key `sync:device_global:{user}`；domain cursor 挂全局点下，不污染每会话游标。verify: `kernel::sync::global_cursor` 2 passed；全量见日志。
- [~] B2 catch-up planner（**多路复用**）— core 侧已补 `GlobalDomainItem` / `route_global_stream(_with_context)`：按 `global_seq` 检测设备级缺口，按 `domain_id` 分桶路由到 `DomainStreamRouter`，领域内按 `entity_seq` 有序应用。未完成：真实 `SyncSince(globalSeq)` 传输 endpoint 接入、`im.messages` runtime applier 收编、未读重算挂接。verify: `kernel::sync::domain_registry` 5 passed。
- [~] B3 服务端 endpoint（flare-im-core）— 服务端 `MultiConversationSync` handler 已存在；本轮完成客户端 B3-2/B3-3 batch 接线，使 changed set 走一次批量拉取。剩余：可选 `SyncSessionHints.{server_user_version,changed_conversation_ids}` 填充，免摘要 diff。参考 `flare-im-core/docs/2026-07-01-server-global-sync-point-B3.md`。
- [x] B4 降级路径 — `MessagesSyncTask` 已从扁平 per-conversation 并发改为 changed-only + attention ordered targets + `MultiConversationSync` 批量 catch-up，最高优先前台会话独立先跑，其余按有界批处理。verify: `sync_task::messages` 3 passed、`sync_protocol_adapter` 6 passed、web E2E messaging flow 1 passed。

### Phase C — 注意力优先级调度器（取代扁平轮询）
- [~] **C1 ConvergencePriority 调度器** — **策略核心已落**（`kernel/sync/scheduler.rs`）：`AttentionState{foreground/visible/recent}` + `priority_for`（P0 前台 > P1 可见 > P2 近期 > P3 长尾/全局）+ `ConvergenceScheduler` 有界优先队列（去重、`pop_next` 按注意力弹最高优先、满时高优逐出低优的背压）。6 单测（打分/前台优先/开会话置顶/背压逐出/去重/关闭清前台）。410 passed。**剩余**：异步 drain worker（真调 domain pull/apply、apply 分片不饿死渲染）+ 接 last_message_ts recency 喂入——待领域/ B 喂数据。
- [x] C2 删除旧路径 — 已移除 `MessagesSyncTask` 扁平 `buffer_unordered` 路径、`spawn_foreground_sync_worker` 周期前台轮询、启动期 `backfillVisibleHistories: true` 默认/调用。verify: `rg "spawn_foreground_sync_worker|FOREGROUND_SYNC|foreground_sync_interval_for_profile|backfillVisibleHistories: true|backfill_visible_histories: true"` 无匹配；`client::im_client` 15 passed。
- [~] C3 注意力信号 API — **Rust 已通**：`AttentionRegistry`(线程安全共享注意力) 挂 `SyncManager`；`IMClient::{open_timeline, close_timeline, set_visible_conversations}`（best-effort，未连接 no-op，attention 仅 hint）；`MessagesSyncTask` 用 `attention.snapshot()` 播种调度器 → **前台优先收敛运行期生效**。413 passed。**剩余**：跨语言契约(bindings→packages→examples，让 app 在 timeline-open 时调用)；presence/typing + 媒体预热复用同一可见域。
- [x] C4 已读上报增量化 — `push_local_read_states(convs, delta_only)`：周期 `ReadStatesSyncTask` 走 delta（`read_ack:{conv}` 游标记已确认位点，仅上报前进过的会话，O(N)→O(delta)）；冷启/重连**拉摘要前的强推保持全量**(correctness)。读态最终一致，偶发跳过下次读操作自愈。413 passed。
- [~] C5 契约传播 — **注意力闭环已在 core 内自动打通（免跨仓）**：把 attention 信号挂进**现有** `ViewApi::open_timeline`（app 打开会话本就调 `timeline.open` binding）→ 打开会话即自动标前台 → 下次收敛/重连优先该会话。**零 app / 零 bindings 改动**。`IMClient::{open_timeline,set_visible_conversations}` 仍保留供更细控制。413 passed。**剩余**（可选优化）：conversation_list 视口 → set_visible、timeline 关闭 → close_timeline、态事件跨语言渲染。

### Phase D — 扩展性、缺口统一与验证
- [x] **D1 调度分类法固化** — `LaneSpec` 已含 `phase × priority × visibility × trigger`；`SyncDomain::lane()` 使用该声明，`add_sync_task` 保留旁路。verify: `lane_spec_declares_trigger_dimension`。
- [~] D2 缺口修复扩展 — 全局流 route 已报告 `missing_global_seqs`（有界 128）并保留领域 `deferred_seqs`；未完成：把报告接入真实 `seq_repair` runtime。verify: `global_stream_reports_global_gaps_without_blocking_known_domains`。
- [x] D3 媒体注意力预热 lane — 新增 `MediaWarmupPlan`：输入 `AttentionState` + media candidates，输出 bounded、按前台/可见/近期排序、按 `file_id` 内容去重的预热计划；预热最高 clamp 到低清，避免偷跑全量下载。verify: `kernel::sync::media_warmup` 2 passed。
- [~] D4 验证 — core 覆盖已完成：全局流缺口/乱序、玩具 `SyncDomain` route、已读 delta、旧路径 grep、热启校准状态事件、冷热启动等待报告、WASM build、web/tauri 自动验证。未完成：真实三端弱网高频切换、原生窗口人工验收。当前最大验证：`cargo test --features storage-sqlite,lifecycle-sqlite` → **461 passed, 5 ignored**；web E2E 1 passed；web app 27 passed + build；tauri test 6 passed + typecheck。

#### 冷/热启动同步等待报告（SDK 确定性用例）

`StartupSyncTiming` 按同一 `run_id` 记录 `LocalReady` / `ForegroundFresh` / `Converged`，并把热启动校准等待定义为 `Converged - LocalReady`。当前确定性测试报告如下（用于守住事件语义和等待口径，非真实设备 P95）：

| 场景 | LocalReady 等待 | ForegroundFresh 等待 | Converged 等待 | HotCalibration 等待 |
| --- | ---: | ---: | ---: | ---: |
| 冷启动 | 20ms | 260ms | 640ms | N/A |
| 热启动 | 5ms | 70ms | 180ms | 175ms |

热启动结论：首屏 T0 仍以本地数据 5ms 出图，不等网络；校准/补齐在后台 NonBlocking 运行并发出同步状态，直到 180ms 收敛。

### Phase E — 领域无关收敛内核 SPI（让群/好友/任意业务复用核心同步）
> 本 Phase 把"消息专用同步"提升为"通用同步内核"。与 B/C 交织：B 的全局流从首日多路复用、C 的调度单元领域无关。core 只提供机制，好友/群语义在 Social 实现并注册。
- [x] **E1 `SyncDomain` SPI** — `kernel/sync/domain.rs`：`SyncDomain{id()->DomainId(开放集), lane()->LaneSpec, pull(since)->DomainDelta, apply(item)->ApplyOutcome}` + `DomainPhase/ConvergencePriority(P0..P3 派生 Ord 前台优先)/DomainCursor/DomainItem`；经 kernel + **`src/spi`** 稳定导出，business SDK 只依赖 spi。3 单测（优先级排序/开放集哈希/游标）。400 passed。
- [~] E2 打开三处硬耦合 — 已完成 (b) `DeviceGlobalSyncPoint` per-domain 子游标、(c) `SyncScope::Domain(DomainId)` 开放集、以及 `SyncDomainContext{user_id,store,bus}` 为 runtime applier 收编铺路；未完成 (a) `SyncEventApplier` runtime 注入到 `im.messages` 内置领域。verify: `kernel::sync::task` 6 passed、`kernel::sync::global_cursor` 2 passed、`kernel::sync::domain` 10 passed。
- [x] E3 `DomainStreamRouter` — `SyncDomainRegistry` 已支持 `route()` / `route_with_context()` / `route_global_stream()` / `route_global_stream_with_context()`；全局流先按 `global_seq` 检测缺口，再按 `domain_id` 多路复用，领域内按 `entity_seq` 有序应用。verify: `kernel::sync::domain_registry` 5 passed。
  - 排序决定：E3(加性 router)先于 E2(拆硬耦合)——在全局流 B 未喂数据前拆除正在工作的 `SyncEventApplier` 会破坏消息同步（correctness>一切）。
- [~] E4 内置领域收编 — 已补 `SyncDomainContext`，解决 builder 期领域没有 user/store/bus 的 runtime 上下文问题；未把正在工作的 `SyncEventApplier` 硬拆进领域，避免在服务端全局流未就绪前破坏消息同步。后续在 B3 endpoint 可用后收编 `im.messages` / `im.conversations`。
- [~] E5 注册 SPI 打通 — **Rust API 已通**：`IMClientBuilder::add_sync_domain(impl SyncDomain)` / `add_sync_domain_arc` → 经 `IMClientExtensionComponents`(含 login 子引擎合并保留) → `SyncManager::register_domain` → `SyncDomainRegistry`。`registered_domain_ids()` introspection。8 处 plumbing + 1 单测（注册 social.friends 领域→可见）。404 passed。**剩余**：跨语言契约 core→bindings→sdk-spec→packages（JS/Swift/Kotlin 侧注册，跨仓）。
- [blocked(cross-repo)] E6 参照领域（在 Social，非 core）— `social.friends`(好友关系/申请) + `social.groups`(群目录/成员) `impl SyncDomain` 注册进内核，证明群/好友复用冷/热/重连/追赶/优先级/缺口。**core 不引入好友/群业务规则。**

### Phase F — 广播/频道读扩散（客户端切片 + 服务端里程碑）
> 大群/频道走 fan-out-on-read 共享时间线；小群保持 fan-out-on-write。百万级读扩散主体在服务端，SDK 消费共享时间线秒开+流畅。频道=会话领域的 sync-strategy 属性，不新建领域。
- [x] **F1 fan-out 模式决策** — `ConversationFanoutMode` + `ConversationType::fanout_mode(_with_threshold)`：小群 fan-out-on-write，大群阈值后 fan-out-on-read，Channel/Broadcast 固定 fan-out-on-read，server hint 可覆盖。
- [~] F2 共享时间线消费 — core 策略层已有 `ConversationTimelinePolicy{shared_timeline, materialize_member_table=false}` 与 B2 全局流 route；未接真实频道服务端共享 timeline。
- [~] F3 订阅/退订模型 — `ConversationTimelinePolicy` 标记 `attention_scoped_subscription` 与 `member_presence_aggregation`；未接真实订阅/退订 transport。
- [blocked(cross-repo)] F4 服务端读扩散 endpoint（flare-im-core 独立仓，并行）— 广播 fan-out-on-read + 共享时间线拉取契约；与 memory 里"统一读扩散"对齐。**协调 flare-im-core 仓。**
- [~] F5 验证 — core fan-out 策略单测 + web messaging E2E 通过；10 万+频道滑动/内存需服务端读扩散和数据集，blocked(cross-repo/env)。

### Phase G — 完整媒体管线（渐进 + 断点续传 + 转码 + CDN）
> core=策略/状态机，packages=平台适配器，examples=渐进态渲染，服务端+基础设施=转码/CDN/签名URL。全程脱离渲染帧。
- [x] **G1 渐进呈现模型** — 新增 `MediaProgressiveState` / `MediaProgressiveStage` / `MediaProgressiveEvent`：占位→blurhash→thumbnail→low→full 单调升级，降级事件被忽略。verify: `domain::media_progressive` 2 passed。
- [x] G2 下行断点续传 — 新增 `MediaDownloadManifestVo` / `MediaDownloadPartVo` / `DownloadManifestState`，对齐上行 part/offset/sha256，支持 monotonic progress、resume offset、all-parts completed 才提交 final path。verify: `domain::download_manifest` 3 passed。
- [x] G3 内容寻址有界缓存 — **已存在**：`media_cache_repo.rs::trim_to_max_bytes`(写入后按 `max_bytes` LRU 驱逐) + `MediaCacheAdmin::set_media_cache_max_bytes`。有界内存预算已满足，无需新增。
- [blocked(packages/env)] G4 传输/解码适配器下沉 packages — core 已保持策略/状态机边界；平台下载、解码、缩略图/blurhash 生成需要 packages 真实适配与 example UI 改造。
- [x] G5 注意力预热接线 — `MediaWarmupPlan` 复用 C1 `AttentionState`，前台/可见/近期优先，bounded + content-addressed dedupe。
- [blocked(cross-repo/infra)] G6 服务端+基础设施里程碑（跨仓/跨栈，并行）— 转码(多清晰度)、CDN 源站、签名 URL/鉴权、缩略图/blurhash 生成侧。**协调 flare-im-core + infra。**
- [~] G7 验证 — core 渐进态、下行续传、媒体预热、缓存既有测试通过；弱网真实中断续传和渐进渲染不卡帧需 packages/example + 服务端媒体能力。

### Phase 验证门映射（Gate，红灯不推进）
- A `[verify]`：`cargo test` 绿；冷/热/重连分流单测；重连不丢 pending 单测。
- B `[verify]`：`cargo test` + 契约 golden；catch-up O(增量)断言；多路复用 router 单测。（B3 必须路径已走 MultiConversation batch；server hints 为可选增强。）
- C `[verify]`：调度器优先级/背压单测；旧扁平轮询+周期前台轮询已删（grep 归零）。
- D `[verify]`：预算测量断言(首屏<200ms/冷启<500ms/重连不阻塞)；玩具 `SyncDomain` 走完冷热重连缺口。
- E `[verify]`：内置领域收编回归不变；`add_sync_domain` 契约 golden；spi 编译面稳定。
- F `[verify]`：小群路径回归；大频道消费单测；(F4 跨仓=blocked 时标注)。
- G `[verify]`：续传状态机单测；缓存 LRU 驱逐单测；渐进态事件序单测；(G6 跨仓=blocked 时标注)。
- **H = 三端人视角总验收**（下）。

### Phase H — 端到端验收（computer-use / browser 驱动三端，账号 11~20）
> 每条 sdk 改造落地后用真实三端 app 做人视角验收。核心断言=秒开秒展示 + 流畅 + 数据不阻碍体验。web→claude-in-chrome；tauri/flutter→computer-use(`request_access` 后)。
- [x] H0 环境拉起 — 后端已由用户启动：60051(ws)/50050(http)/60084(sync-orchestrator)/60060(grpc) 全 OPEN；flutter 3.44.1 已在 PATH；dev-token secret 就位。本轮已重建 WASM 并同步到 web/tauri/electron 副本，web Playwright E2E 已复验。
- [~] H1 冷启动 — SDK 等待口径已落地并有确定性报告：LocalReady 20ms / ForegroundFresh 260ms / Converged 640ms；真实清本地后三端设备验收仍需环境。
- [~] H2 热启动 — SDK 等待口径已落地并有确定性报告：LocalReady 5ms / ForegroundFresh 70ms / Converged 180ms / HotCalibration 175ms；热启动校准会触发同步状态但不阻塞本地优先首屏。真实二次启动三端设备验收仍需环境。
- [~] H3 1:1 互发 — web Playwright business flow 通过（双用户发送、未读/已读、reaction、reply、copy、edit、delete、recall、pin、refresh）；三端原生/弱网仍 blocked(env)。
- [blocked(env)] H4 断线重连 — 发送中途切网/关网再恢复：pending 不丢、CatchingUp 静默、追赶补齐无卡顿；高频切网×N 稳定。
- [~] H5 注意力优先级 — core 调度/批量 catch-up 单测通过；真实 20+ 会话人视角 blocked(env)。
- [~] H6 多端收敛 — web E2E 覆盖 reaction/edit/recall/read 等业务流；web+tauri 双端人工 blocked(env)。
- [blocked(cross-repo/env)] H7 频道读扩散(F) — 建 broadcast，11 发、12~20 经共享时间线收；大频道滑动 60fps、内存有界。
- [blocked(packages/env)] H8 媒体管线(G) — 11 发图/文件给 12：渐进呈现(占位→blurhash→清晰)不卡帧；弱网上/下行中断可续。
- [~] H9 记录 — 本轮命令验证已记录在 Execution log；三端截图/录屏 blocked(env)。

### Phase I — 深度优化波（2026-07-02 架构评审 10 项；正确性 > 等待 > 效率 > 及时性）
> 来源：2026-07-02 顶级 IM 架构师评审（对齐飞书/Telegram 标尺）。协议资产盘点：`SyncStaleContext{server_earliest_available_conversation_seq,recommended_action}` + `SYNC_RECOVERY_HINT_REFETCH_SNAPSHOT` + `SyncSessionHints{server_user_version,changed_conversation_ids,user_version_index_truncated,server_max_conversation_seq}` + `SyncSnapshotSync`（摘要+消息内嵌 bundle）**已存在于 flare-proto**——I2/I4/I6/I10 优先复用，不新造协议；确需新字段一次性补进 flare-proto 并重生成。
- [x] **I1 游标-落盘顺序契约（数据不丢 P0，最高优先）** — 审计结论：push 路径安全（`persist_durable_batch` save_batch 成功后才 `repair_message_seq_after_persist` 推游标，失败游标不动）；catch-up 消息路径安全（save_batch `?` 传播）；**真实缺口**=`apply_single_conversation_page` 用**解码** seq 推游标而丢弃 `apply_events` 结果 → 应用失败（Ok(false)/Err，日志称"保留重放机会"）的事件被游标越过=永久丢失。修复：`apply_events` 改返回与入参对齐的 `Vec<bool>`；游标只计入 covered_item_seqs（消息/skip/tombstone，落库有保证）+ 应用成功的事件 seq；失败事件形成 has_seq_gap 走既有补拉重放。`DeviceGlobalSyncPoint` 落顺序契约文档不变量。产出：models/decoding/event_applier/mod.rs 改造 + 新测试 `sync_page_cursor_stops_before_failed_event_seq`。verify: `cargo test --features storage-sqlite,lifecycle-sqlite` → **462 passed, 5 ignored**（461→462）。
- [x] **I2 gap-too-large 快照切换路径（数据不丢 P0）·客户端** — 落地：`snapshot_cutover_required()`（stale.recommended_action / hints.recovery_hint ∈ {REFETCH_SNAPSHOT, RESYNC_CONVERSATION}）+ `request_sync_snapshot`（复用服务端已有 SyncSnapshot handler）+ `SyncApplyUseCase::apply_snapshot_row`（消息落库+权威已读/未读覆盖，先数据后游标守 I1）+ `resync_conversation_from_snapshot`（游标置快照水位，旧历史留 backfill）。触发点：`apply_sync_res_single`（消息页）+ `sync_critical_events`（事件回放缺口，事件游标跳快照水位）。发现并记录：**retention purge 场景下 single/multi conversation 消息路径服务端不填 stale → 客户端 contiguity 卡死永不收敛**，快照切换只有在服务端发 stale 才触发 → 服务端填充列入 J1 必做。`user_version_index_truncated`（用户级截断）归 I4 版本快路径一并处理。verify: `snapshot_cutover_triggers_on_stale_or_hint_recovery` + `snapshot_row_apply_persists_messages_and_returns_cutover_seq` 绿；全量 **464 passed**（462→464）。
- [x] **I3 全局流安全水位（背压拒绝/缺口截断不忘，正确性）** — **方案修正**（比持久欠账簿更优更简）：(a) scheduler 满拒无需账簿——MessagesSyncTask 每轮从服务端摘要重新 diff 变化集（容量=total 不拒），被拒/失败目标下轮自动重发现，触发兜底由 I10 水位对账保证；(b) 真正会忘的是全局流：新增 `GlobalStreamRouteReport.safe_global_seq` = **连续∩应用成功前缀水位**（缺口或已注册领域 Deferred 即停；unrouted 不阻塞），设备全局游标只准推到 safe——`missing_global_seqs` 截断从此无害（下次从 safe 重拉自动重检全部缺口）。与 I1 的 per-conversation contiguity 语义完全一致。verify: 新增 3 测试（deferred 阻塞 / unrouted 不阻塞 / 截断无害）+ 既有 2 测试补 safe 断言；全量 **467 passed**（464→467）。
- [x] **I4 重连零增量快路径（降等待）** — **方案修正**（勘察后发现无需 user_version 新协议）：会话列表同步已是增量探针（送 `CONVERSATION_CURSOR_KEY` ms 游标，零变化=空响应 1 轻量 RTT，服务端 `conversations_sync_echoes_client_cursor_when_no_changes` 已保证）；真正的重连 O(N) 元凶是两个：(a) **`sync_critical_events` 每轮对全部会话逐发 QueryEvents** → 加 `critical_events_provably_clean`（事件与消息共享 conversation_seq，摘要水位≤已检查位点⇒可证明无新事件跳过）+ 拉到服务端水位后把检查位点抬到摘要水位 → QueryEvents O(会话数)→O(变化)；(b) **重连/热启前的已读强推是全量 O(N) ack**（每次切网 ack 风暴，历史教训 ACK 116s）→ 恒 delta 一条路径（删 `delta_only` 参数与全量模式；位点 send 成功才记、live 路径不记位点必然重推；残余窗口=他端未读徽标暂滞、下次阅读自愈）。零增量重连成本现为：会话列表 1 空响应 + 0 消息请求 + 0 QueryEvents + 0 ack。`server_user_version` hints（B3-4）继续保持可选增强不做。verify: `critical_events_skip_requires_known_watermark_at_or_below_checkpoint` 绿；全量 **468 passed**（467→468）。
- [x] **I5 open_timeline P0 即时补拉（降等待）** — `ViewApi::open_timeline` 标注意力的同时 `spawn_foreground_pull`：fire-and-forget `SessionSyncRunner::request_message_sync`（本地快照即时返回不被网络阻塞）；`ForegroundPullGuard` single-flight 同会话在途去重、完成释放；未接线/失败静默降级为纯 attention hint；与批量收敛重叠由 LWW/converger 幂等兜底。attention 从"hint 等下一轮"升级为"P0 中断"。verify: `foreground_pull_guard_dedups_in_flight_and_releases_on_finish` 绿；全量 **469 passed**（468→469）。
- [x] **I6 冷启 bootstrap bundle + 会话列表增量真修复（降等待，解 A2 blocked(protocol)）** — **执行中发现列表增量从未生效的双侧格式断裂**：服务端游标格式 `ms|cid`（`parse_snapshot_cursor` 要求 `|`，纯 ms 当冷启全量）+ `!has_more` 时回显旧游标；客户端发纯 ms + `next_cursor.parse::<u64>()` 对 `ms|cid` 必得 0 → **游标永不前进，每次热启/重连全量拉列表**。修复（proto+server+client）：①proto `SnapshotConversationRow.summary`(bundle 内嵌完整摘要) + `SyncSnapshotSync.{newest_first,conversation_page_limit}`(降序首屏 top-N / 页大小与每会话消息数正交)；②server `parse_snapshot_cursor` 容错纯 ms、页非空即返回行水位游标（增量真生效）、**消息查询移到分页裁剪之后**（消灭全账号每会话查 limit 条再丢弃的 O(N) DB 放大）、`conversations_sync` messages_per_conversation 100→1（摘要只需最新 1 条做预览）、include_conversations 时填 row.summary；③client `snapshot_cursor_watermark_ms`（取 `|` 前 ms）+ `cold_start_bundle_sync`（无游标→降序分页 bundle：50 会话/页×30 消息，摘要+消息一次 RPC 到位，页内落库后推会话游标(I1)，**列表水位整个 bundle 完成才保存**防降序中途崩溃产生永久缺口）。冷启网络轮次：1 RPC/页（原 2+）；热启/重连列表：全量→O(变化)。verify: server `cargo test -p flare-sync-orchestrator` **20 passed**；sdk 全量 **470 passed**（469→470，含 `snapshot_cursor_watermark_extracts_ms_from_paging_and_plain_forms`）。
- [x] **I7 批量 start_positions 批量读（效率）** — trait 层：`SyncCursorReader::get_conversation_cursors` / `ConversationReader::get_many` / `ConversationWriter::get_local_max_seqs`（默认逐条回退，memory/indexeddb/empty 免改）；SQLite 覆盖为单条 IN / GROUP BY 查询（500 分块守绑定变量上限）；`multi_conversation_start_positions` 3×N 串行 await → 3 次批量调用，语义（cursor/cleared_floor/local_max 合成 start_seq）不变。verify: `batch_cursor_read_matches_single_reads` 绿；全量 **471 passed**（470→471）。
- [x] **I8 批间重排 + P0 抢占（效率/等待）** — `MessagesSyncTask` 从"一次性规划全部批次"改为 `next_batch_plan` 循环：每批开始前 re-snapshot `AttentionRegistry`（合并本轮 recency 标记）重排剩余目标；头部为 P0 前台 → 单飞抢占（最小 RTT 到屏），否则有界成批。删旧 `conversation_batches_from_ordered_targets`。verify: `foreground_conversation_preempts_next_batch_alone` + `mid_run_timeline_open_reorders_remaining_batches` 绿；全量 **472 passed**（471→472）。
- [x] **I9 自适应批量/分页 + apply 分片让帧（效率/流畅）** — `AttentionState.app_in_background`（`set_heartbeat_app_state` 接线，平台生命周期→收敛降配）+ `adaptive_sync_budget`（后台批/页减半、下限 1，随 I8 批间快照逐批生效、回前台即恢复）+ `yield_for_frame`（WASM=setTimeout(0) **宏任务**让浏览器渲染，原生=yield_now）挂到 multi-conversation 每页 slices 后与冷启 bundle 逐行后。verify: `background_budget_halves_batch_and_page` 绿；全量 **473 passed**（472→473）。
- [x] **I10 在线水位对账（及时性）·核实为已存在** — 勘察结论：机制已在先前波次完整落地且两侧有测试——客户端 dispatcher `waterline.rs` 三源触发（`sync.waterline` 控制包 / `EventEnvelope.max_conversation_seq` / 单事件 seq）+ 合并去抖 single-flight + 本地已达检查（`empty_event_envelope_waterline_triggers_message_sync` 等测试在）；服务端 gateway 填 envelope 水位并支持纯 ping（`pure ping requires max_conversation_seq` 校验+测试）。评审时"在线无实时水位校验"的前提过时；真正缺的连续/截断安全水位已由 I3 补齐。**遗留（超出本轮授权仓）**：连接健康但事件全静默丢失的极端场景需心跳捎带水位（flare-core transport）或服务端周期纯 ping 策略——记入 Notes 待跨仓协调。verify: 既有 waterline 测试绿（本轮全量回归含）。
- **I gate `[verify]` ✅ 2026-07-02**：`cargo test --features storage-sqlite,lifecycle-sqlite` → **473 passed, 5 ignored**（基线 461→473，+12 新测试）；`cargo test -p flare-sync-orchestrator` → **20 passed**；proto 双侧编译绿。I1 ordering / I2 快照切换 / I3 safe 水位 / I4 changed-only+delta / I5 single-flight / I6 bundle+游标修复 / I7 批量读 / I8 抢占 / I9 降配+让帧 全部单测绿。

### Phase J — 跨仓优化扫描（用户 2026-07-02 授权：有问题直接改，不逐项询问）
- [~] **J1 flare-im-core 服务端同步链路扫描** — 已落地：①`get_sync_snapshot` 消息查询移到分页后 + newest_first + row.summary + 水位游标（随 I6）；②`conversations_sync` messages_per_conversation 100→1；③`multi_conversation_sync` 批内查询串行→**保序有界并发**（buffered(8)，64 会话批延迟 Σ→≈max×N/8）；④**重要勘察修正**：`build_contiguous_sync_items` 已用 tombstone 填满每个 seq 空洞（single+multi 同构造器）→ I2 担忧的"retention 空洞客户端 contiguity 死锁"不成立，服务端 stale 补填**无需做**（stale 已在 query_events 缺口路径正确填充）；⑤顺手修 arch 守卫：16 个未登记 env var（并行会话 WIP）登记进 env_registry + gateway `conversation_subscription.rs` 8 处 `.expect(` 改毒锁恢复（长驻路径不 panic）。剩余：workspace 全量 test 复跑确认（分类器恢复后）。
- [~] **J2 flare-im-core-client-sdk 扫描** — 勘察：ts-sdk `views.openTimeline` 契约已生成且 Vue UI 走 `client.views.openTimeline`/close（MESSAGE_PAGE_SIZE=40 已调优）→ I5 即时补拉对全平台**零改动生效**；发现并修复 core 侧 C5 遗留缺口：**`ViewApi::close` 不清 attention 前台 → 已关会话永远 P0 持续错抢收敛优先级**，已改为关时间线即 `close_timeline`。web bridge 无串行 await N+1（invoke 单发带超时）。剩余：wasm 重建后 packages/web 回归（J4）。
- [x] **J3 flare-proto 缺口一次性补齐** — 本波新增字段已随 I6 落地：`SnapshotConversationRow.summary`、`SyncSnapshotSync.{newest_first, conversation_page_limit}`；flare-proto `cargo build` 绿，服务端/客户端双侧编译绿。无其他缺口（水位/恢复字段先前已存在且本波确认服务端真实填充）。
- [x] **J4 全量回归** — core-sdk **473 passed, 5 ignored**；flare-im-core workspace 全绿；`cargo xtask build wasm` 重建成功并同步 web/tauri/electron 副本（`shasum` 全部一致）；ts-sdk **76 passed**；vue-im-ui **109 passed** + `vue-tsc` 绿；web app **27 passed** + `npm run build` 绿（bundle budget OK）；tauri `typecheck` 绿 + **6 passed**。（执行中段遇 Bash 安全分类器中断约 20 分钟，恢复后补齐。）

### Phase K — 第二轮深度审查（2026-07-02 下午：热路径性能 + 稳定性）
> 审计范围：发送/接收热路径、事件总线、persist worker、视图刷新、SQL 配置/索引、有界性/背压/泄漏、服务端 bootstrap 链路。**审计结论先行**：EventBus 有界+滞后标记、可靠队列自适应 in-flight、SQLite WAL+busy_timeout+cache/mmap、converger 批量查、seq_repair/waterline 状态有界剪枝、服务端 cursor cache FIFO 封顶——这些均已达标无需动。以下为真实缺陷/优化及落地：
- [x] **K1 事件总线订阅过滤（接收热路径分配）** — 3 个 session watcher 只消费 Connection 事件却无过滤订阅：每条事件（含 128 条消息的 `ReceivedBatch`）被深克隆进其队列后丢弃。改 `subscribe_filter(SdkEventKind::Connection)`（过滤在克隆**之前**）。
- [x] **K2 publish 免克隆订阅者表** — `publish_to_subscribers` 每次 publish 克隆整张订阅者 Vec（含 EventFilter 内 String）；sends 全程同步 try_send 不 await → 改持读锁遍历，零克隆。
- [x] **K3 同 phase 共享会话列表快照** — Background 阶段 messages/read_states/settings/key_events 四任务并行各自 `conversations.list()`（latest-visible-message JOIN 重查询）→ `SyncContext.conversations_snapshot()`（`SharedConversationsSnapshot` 惰性 OnceCell），×4→×1 且同 phase 快照一致；`sync_critical_events` 改由调用方传入。
- [x] **K4 远端游标推送去放大（catch-up 隐藏大头）** — 每次会话游标推进=2 个内联等待 RPC（GetSyncCursor 预读+UpdateSyncCursor，预读缓存 miss 还触发服务端全量 bootstrap）→ ①删预读（服务端 `update_sync_cursor` 已 merge_cursor_monotonic 单调合并，有测试守护）；②推送改 `RemoteCursorFlushState` 合并（同会话取 max）+200ms 去抖后台 worker 批量推（advisory 数据不占 catch-up 延迟；worker 未装载时内联回退语义等价；Weak 持有防泄漏）。50 会话 catch-up 的游标 RPC：100 个同步等待→0 个热路径等待+≤50 个后台合并推送。
- [x] **K5 服务端 bootstrap 分页缓存** — `get_sync_snapshot` 每一页都重跑全量 bootstrap（全账号 3×LATERAL）：冷启 bundle 10 页=10 次全量。新增 `BootstrapPageCache`（续拉页专用：首页恒新鲜并回填；cursor 非空页在 TTL 内复用同一快照——既消灭 O(页数×全账号 LATERAL) 放大，又让同一分页序列看到一致数据集）；TTL `SYNC_ORCHESTRATOR_BOOTSTRAP_PAGE_CACHE_TTL_MS`（默认 1500ms，0=禁用），容量 4096 先清过期再整清；env var 已登记。
- [x] **K6 ViewApi::close 清注意力前台**（随上轮 J2 落地，归档于此）。
- **K gate `[verify]` ✅**：sdk **473 passed, 5 ignored**；flare-sync-orchestrator **22 passed**（+2 缓存测试）；flare-im-core workspace 全绿；WASM 重建+5 副本 hash 一致；web **27 passed**+build budget OK；tauri typecheck+**6 passed**。
- 未采纳/记录：视图 worker 需广谱事件（不过滤）；`get_remote_cursor_seq` 保留给 CONVERSATION_CURSOR_KEY 选择路径（每 run 一次）。
- [x] **K7 bootstrap 存储层增量过滤（updated_after，原"下一步服务端里程碑"）** — 链路：proto `ConversationBootstrapRequest.updated_after_ms=7`（flare-grpc-proto）→ flare-conversation grpc/query/handler/domain → `ConversationRepository::load_bootstrap(+updated_after_ms)`：Postgres 加可下推 WHERE（`s.updated_at/sp.joined_at/sp.updated_at > $3 OR EXISTS(messages.timestamp > $3)`，EXISTS 走既有 `idx_messages_conversation_ts`，不引用 LATERAL 输出故在 LATERAL 前裁剪行）；Redis 实现按 last_message_ts 对齐语义。orchestrator：**仅 ASC 列表续拉**传边界（`cursor_ms - 1` 保证是分页元组过滤的超集，绝不欠拉）；DESC 冷启 bundle 与定向 conversation_ids/游标回填传 0（需全集）。`BootstrapPageCache` 升级**超集规则**（条目记录过滤边界，只服务边界 ≥ 它的请求；全量条目不被更窄条目覆盖）——热启/重连列表同步的存储成本从 O(全账号会话×LATERAL) 降到 **O(变化)**。verify: orchestrator **23 passed**（+超集规则测试）、flare-conversation **11 passed**、workspace 全绿。
- **发现的既有限制（产品决策，未动）**：`max_bootstrap_conversations` 默认 100——bootstrap 截断 top-100（未读优先+时间排序），>100 会话的账号冷启列表只到 100；需要调大配置或给 bootstrap RPC 加分页才能支撑大账号全量。

### Phase L — /simplify 四仓质量波（2026-07-02 晚：4 并行审查代理 → 去重落修）
> 复用/简化/效率/抽象高度四角审查四仓工作区改动（43 条原始发现，去重后按价值落修）。**最大产出是一个隐藏正确性缺口**：
- [x] **L0（正确性，审查副产物）游标钳制吞掉水位/快照型保存** — `save_cursor_with_remote` 的连续性钳制对**伪 key**（`__conversations__`、`critical_event:*`：无本地消息行→contiguous=0）与**快照建立型游标**（I2 切换/I6 冷启 bundle：本地只有最新页）恒钳到 0 → `safe==0` 早退**静默不保存**——I4 位点抬升、I6 列表水位与 bundle 行游标、I2 切换游标实际全是 no-op（也解释了列表游标从未持久化的历史行为）。修复：拆出 `save_watermark_with_remote`（无钳制；共享 `persist_cursor_and_push_remote`），钳制版仅保留给逐页增量应用；调用点分流；新增 `watermark_cursor_save_persists_pseudo_keys_that_clamped_save_skips` 钉死两条路径分工。
- [x] L1 死管道清除：adapter `init_message_sync_concurrency` 字段/ctor 参数/`_concurrency` 参数/未用 `futures::stream` import（config knob 保留——经 builder 喂 `MessagesSyncTask` 批大小）；`update_remote_cursor_seq` 幽灵 `user_id` 参数（含 worker `""` 调用点）与 `effective` 遗留重命名；`push_local_read_states` 三段叠置过时 doc（描述不存在的 `delta_only` 参数）合并为一段。
- [x] L2 messages 任务：跳过判据的逐会话 `get_conversation_cursor` **N+1**（I7 刚消灭的同款）→ 一次 `get_conversation_cursors` 批量；水位判据两处拼写 → 上提 `kernel::watermark_provably_clean` 共用；recency 播种两处 → `seed_recency` 闭包；`multi_conversation_batch_size` 双重 normalize 去除；`next_batch_plan` 排序键逐元素 String 克隆 → `AttentionState::priority_for_conversation(&str)`。
- [x] L3 复用抽取：`apply_snapshot_row` 与页应用逐字重复的 25 行落库管线 → `persist_converged_messages`；关键事件检查点保存重复块 ×3 → `save_critical_checkpoint`；`single_response_from_multi_slice` 改按值 move（整页 protobuf items 不再克隆，批量 catch-up 热路径）；manager.rs 10 处毒锁 match → `lock_recovered` 单行。
- [x] L4 服务端：`get_sync_snapshot` 分页重构为**引用级**筛选/排序/分页、只克隆最终页（K5 缓存命中的续拉页不再 K×N 克隆）+ 页内消息查询保序 `buffered(8)`（与 multi 同款，常量上提共用）+ 游标单次解析 + `(ms, cid.as_str())` 免克隆比较；`MemorySyncCursorCache` Default/new 分叉（Default 丢 env 容量覆盖）→ 同路径；postgres `take(10)` 硬编码 → `MEMBER_PREVIEW_LIMIT`。
- [x] L5 幂等键空间统一（storage/writer）：tenant/sender/conversation 作用域规则 4 处重复（trait 默认 ×2 + Redis 覆盖重建）→ 唯一 `scoped_client_idempotency_key`，Redis 覆盖删除（最终 key 逐字不变，测试断言键空间等价）。
- **L gate `[verify]` ✅**：sdk **474 passed, 5 ignored**（+1 watermark 回归）；flare-im-core：orchestrator 23、conversation 11、storage-writer 24、workspace 除一个**既有 flake**（`encode_prometheus_text_exports_registered_metrics` 并行跑全局 prometheus 注册表冲突，隔离运行绿，与本波无关）外全绿；WASM 重建 5 副本 hash 一致；web 27+build budget OK；tauri typecheck+6。
- **跳过（记录，不落修）**：①可靠队列 `awaiting_durable` 与 in-flight 机制合并（stage 枚举化）——并行会话在途 WIP 的正确性敏感重构，留给其归属会话；②`"before:"` 字符串化回溯协议跨三层重设计（应为 proto 显式 direction 字段+DESC 索引查询）——候选下一波协议工作；③`DeliverToConversationRequest` 改 oneof——同上；④3 个手卷有界缓存换 moka——现实现有测试且效率代理认可，留作将来合并；⑤env 读取移出 domain/application ctor——涉及两服务 bootstrap 接线；⑥`local_materialized_contiguous_seq` 全量历史回扫改窗口校验（Eff#2）——正确性证明机制变更，需独立会话带崩溃测试做；⑦read_ack delta 的 get_raw N+1 批量化、`has_any()` 探测、dispatch 双克隆 EventDedupeKey 化、engine bootstrap 双胞胎合并、IN 分块占位符 helper、timeline 10k 窗口增量合并、CapturingProducer 测试件上提——均为有效发现，逐条记录待后续小批落地。

### Phase M — /simplify 跳过项落地续批（2026-07-02 深夜）
- [x] **M1（正确性+性能）游标连续性证明 floor 有界化**（原 Eff#2）— `save_cursor_with_remote` 的证明下界改为 `max(旧游标, 清空水位)`：①归纳正确（旧游标保存时其下已证明）；②修复同族缺口——**本地清空过历史/快照建立后的会话，从 0 证明恒断裂 → 游标永久卡死**；③每页保存的扫描成本 O(全历史)→O(本页窗口)。`local_materialized_contiguous_seq(+floor_seq)`；新增 `clamped_cursor_save_proves_only_window_above_prior_cursor` 回归。
- [x] M2 `has_any()` 存在性探测（启动冷/热分类不再拉整张 JOIN 列表；SQLite EXISTS 覆盖）。
- [x] M3 read_ack delta 批量化：`SyncCursorReader::get_raws`（默认逐条 + SQLite IN 覆盖）；`push_local_read_states` 预载一次，删 `read_ack_advanced` 逐会话点查（重连/热启 N 点查→1 批量）。
- [x] M4 接收热路径双克隆消除：EventEnvelope 消息事件 payload 按值 move（原 message+Event 双份深克隆）；失败回滚只留小 `EventDedupeKey`（`EventDeduper::{key_for, forget_keys}` 一次锁内批量回滚）；`dispatch_inbound_message_event_batch` 复用 `persist_durable_batch`（改返回 durable bool），删除 forget 循环 ×3 与内联落库副本。
- [x] M5 engine 收敛：`bootstrap`/`bootstrap_nonblocking` 孪生合并为 `bootstrap_with(nonblocking)`；`ReconnectPlan::ReconnectTransport { transition }` 携带 FSM 转移（决策集中 plan_reconnect，执行侧删二次 match）。
- **M gate ✅**：sdk **475 passed, 5 ignored**（+1 floor 回归）；WASM 重建 5 副本一致；web 27+budget OK；tauri 6。
- [x] **M6 回溯协议字段化（协议级，历史消息加载路径）** — ①proto `SingleConversationSync.before_conversation_seq=5`（方向显式化）；②client 新增 `request_backfill_page`，删 `format!("before:{seq}")` 字符串游标；③orchestrator 按字段分支，删 `backfill_before_seq` strip-prefix helper 与 `build_backfill_sync_items` 的 `"before:{min}"` next_cursor（回溯分页本就由客户端驱动）；④storage-reader `query_messages_by_seq` 的 `ORDER BY CASE WHEN ... THEN -seq` 索引失效 hack → 显式 `ORDER BY seq ASC/DESC`（走 `(tenant_id,conversation_id,seq)` 索引序；format! 中 SQL 字面大括号已转义）。verify: orchestrator 23、storage-reader 11、workspace 无 FAILED；sdk **475**；WASM 5 副本一致；web 27+budget；tauri 6。
- [x] **M7 timeline 热刷新增量合并** — `hot_timeline_snapshot`：key→index 一次构建（原每条 incoming 线性扫描且逐元素重算 timeline_key=O(K×W) String 构造）；常态追加路径改两路有序合并 O(W+K)（`compare_for_latest_window_desc` 为 asc 严格反序 ⇒ "desc截断+回排"≡"asc+截首"）；就地更新/前缀失序回退全量重排保语义（既有测试恰含降序旧快照，防御生效）。修复回退分支 truncated 标志在截断后计算的顺序问题。verify: `hot_timeline_snapshot_merges_out_of_order_incoming_into_sorted_window` + 既有 3 测试绿；sdk **476**。
- [x] **M8 SQLite IN helper 统一** — `sqlite::{SQLITE_IN_CHUNK=500, in_placeholders}`；移植 6 站点（cursor×2/conversation×2/user_repo(900→500)/message_reader×2）。
- [x] **M9 DeliverToConversationRequest oneof 化** — proto 三变体 `payload { messages | events | ping }`（reserved 旧字段 2/4-7），"按字段空/非空推断+纯 ping 哨兵"的暗约定改为互斥且穷尽的显式判别；gateway 解码改 total match（空 messages/空 events/ping 无水位=显式 InvalidParameter）；push-worker 三个构造点按变体构造；grpc 日志按变体输出。verify: gateway+push-worker **75 passed**；workspace 无 FAILED。
- 收口后仍开放（评估为不做/低值）：moka 缓存合并（现实现有测试且有界，合并是纯偏好）；env 读取注入化（涉两服务 bootstrap 接线，等服务配置统一波）；tail 截断 5 行本地惯用法跨 crate 抽取（过度抽象，保留）。

### Phase N — 大账号 bootstrap 上限放开（2026-07-02 收官）
- [x] **N1 max_conversations 请求级上限** — 原 `max_bootstrap_conversations` 默认 100 恒截断：>100 会话账号的其余会话**永远不经列表同步下发**（K7 时记录为产品决策，现以协议解决）。proto `ConversationBootstrapRequest.max_conversations=8`（0=服务默认保守值；>0 受 `BOOTSTRAP_HARD_MAX_CONVERSATIONS=5000` 硬上限钳制）→ conversation 全链传参 → orchestrator 快照分页与游标回填传 `SNAPSHOT_BOOTSTRAP_MAX_CONVERSATIONS=5000`（分页在编排层完成 + BootstrapPageCache 使全集拉取 ≤1 次/1.5s/用户 + updated_after 使 warm 增量只拉变化）。其他调用方（含 get_sync_cursor 之外的默认路径）行为不变。verify: conversation+orchestrator **34 passed**；workspace 无 FAILED。
- 簿记：A2-T1 已由 I6 收口；C1 的"异步 drain worker"被 I8 批间重排+P0 抢占取代（调度单元=批而非单会话，正确性由幂等兜底）——设计意图达成，关闭；C5 的 timeline 关闭→close_timeline 已在 K6 落地。
- [x] **N2 防熵探测（"心跳捎带水位"的正确替代）** — 设计裁决：心跳捎带方案被否——网关**没有源头水位**（消息/事件的权威 seq 在 conversation/storage 服务），逐心跳去查等于把对账成本转嫁给每次心跳，且端到端防熵必须对账数据源头，本质上就是一次轻量增量同步。落地：engine `spawn_anti_entropy_probe`——每 300s 一次摘要级静默对账（`sync_foreground_convergence` = 会话列表增量 + critical events changed-only，经 I4/I6/K7 后**零增量成本=1 个空响应 RPC**）；会话激活装载（重复装载替换旧循环）、注销/断连中止。与 C2 删除的 30s 扁平前台轮询本质不同：周期长一个数量级、成本 O(变化)、语义=兜"连接健康但下行静默丢失且无任何后续触发"的极端窗口。verify: sdk **476 passed**；WASM 5 副本一致；web 27+budget；tauri 6。
- [x] **N3 真实环境端到端复验（H0/H3 刷新到今日协议）** — 自主拉起：podman 既有 infra 容器（consul/postgres/redis/nats/rustfs 直接 `docker start`，绕过 compose 网络标签冲突）→ `FLARE_SKIP_BUILD=1 start_server.sh single`（今日新构建二进制，含全部新协议字段）→ 四服务端口全 OPEN → vite 1430 预热（注意：`npm run dev -- --port` 会被嵌套 npm 吞参落到 5173，须无参启动走 vite.config 的 1430）→ **web Playwright E2E messaging flows 1 passed (16.4s)**：双用户互发/未读已读/reaction/回复/复制/编辑/删除/撤回/pin/刷新全流程，覆盖冷启 bundle(summary/newest_first)、列表增量游标、oneof 投递、水位游标保存、防熵探测装载等今日改动的真实链路。服务端日志：sync-orchestrator/conversation/storage-reader/push-worker **0 ERROR**；access-gateway 仅 1 条同账号双登踢旧连接的良性竞态（send-after-close，与本轮无关）。后端与 vite 保持运行中，可续跑 H4-H8 人工场景。

### Phase O — 四端秒启动（热启动免登录本地出图，2026-07-03）
- [x] **O0 core 缺口修复：prepare 后本地会话未激活** — 原生复现（新增 `tests/prepare_local_first_test.rs`）：`prepare`（本地半段登录）后 `bootstrap_startup_home` 返回 `[NOT_CONNECTED]`——各 API 的 `ensure_session_active` 检查的共享 `current_user_id` 只在 `engine.connect` 成功后写入，"prepare 把本地重活移出登录关键路径"的既有设计从未真正支撑本地优先读。修复：`SdkEngine::adopt_local_session_identity`（prepare 时预写会话身份）+ **连接失败不再清空身份**（库与身份同属该用户，清空反而制造不一致；登出仍经 `deactivate_local_session` 清空）。语义：`current_user_id`=本地会话身份，`connection_state`=网络会话，两轨分离。verify: 新测试绿；workspace **557 passed** 无回归。
- [x] **O1 共享 Vue UI 热启动（web+tauri 一次覆盖）** — `useFlareCoreClient`：`SavedSessionProfile` localStorage 持久化（登录成功保存/logout 清除）+ `resumeSavedSession()` 单飞（init→subscribeEvents→prepare→`bootstrapStartupHome(startBackgroundConvergence:false)` 本地水合→open 会话列表 view→homeSyncReady=true→**后台** connect：token 失效经 `generateCoreToken` 重签单次重试，连上后再跑一次 startup home 收敛+补 messageBuildCatalog；离线保持本地视图）。两 app router `beforeEach` 异步 resume：有档案免登录直进 workbench。verify: vue-im-ui + web + tauri 三处 vue-tsc 绿；tauri src-tauri `cargo check` 绿。
- [x] **O2 web 真实环境 E2E** — 新增 `startup-hot-resume.spec.ts`：登录→互发→reload 热启动（**340ms 会话列表可见**，免登录、列表预览来自本地库）→恢复后双向互发照常→登出清档案 reload 回登录页。**1 passed (6.3s)**；messaging-flows 回归 **1 passed (15.2s)**（其 reload 分支现走热启动路径）。前置：`cargo xtask build wasm` + 中央→web/tauri 四副本重部署 + vite 重启。
- [x] **O3 flutter app 热启动** — SDK 包补齐既有滞后：`_DefaultSyncApi.{bootstrapStartupHome,backfillConversationHistory}` + wire codec 四函数 + `SyncEventName.readiness` switch 分支（此前 app 全量测试编译即挂）。app 侧：`SavedSessionStore`(SharedPreferences) + wrapper 拆 `prepareLocal/connectRemote`（login=两段组合）+ `IAuthRepository/AuthService` 加 `prepareLocalSession/connectSession` + `CurrentUserNotifier.resumeSavedSession()`（单飞；prepare 本地出图→后台 token 重签+connect；偏好存储不可用按无档案降级）+ go_router `/` 异步 redirect + 登录成功持久化/登出清除。verify: `flutter analyze` 0 issues；**62/62 tests**；`flutter build macos --debug` 成功。
- [x] **O4 iOS app 热启动** — apple-sdk 补同款滞后：`DefaultSyncApi.{bootstrapStartupHome,backfillConversationHistory}` + WireCodec 四函数 + events switch `.readiness`。app 侧：`SavedSessionStore`(UserDefaults) + `AppSession.resumeLocal`（create→init→prepare→订阅原生事件→媒体缓存配置，不连网）+ `connectInBackground`（token 重签+connect，失败保持本地视图）+ `FlareAppStore.resumeSavedSession`（resume→section=conversations→bootstrapHome 本地直出→后台连接后再刷诊断/目录/首页）+ `RootWorkbenchView.task` 启动尝试恢复（登录页只在无档案/恢复失败时出现）+ login 持久化/logout+dispose 清除。verify: swift build 绿；**43 tests 0 failures**；iPhone 17 Pro 模拟器 xcodebuild **BUILD SUCCEEDED**。
- 诚实边界：web 端冷/热启动+收发是真实后端 E2E 实测；flutter/iOS/tauri 为编译+单测+模拟器构建级验证（核心链路与 web 共享同一 core/协议），真机/GUI 实测互发属 H4-H8 人工场景（后端保持运行中可续跑）。

## Notes / open questions

- 契约传播顺序不可跳：改 bindings 前先定 core 行为；生成文件不手改。
- 全局流应用**必须**复用 `SyncEventApplier`（LWW + 乱序重放）与 `IncomingMessageConverger`（pending 合并/去重），否则会丢掉现有正确性资产。
- 全局游标 key 与每会话游标/`CONVERSATION_CURSOR_KEY` 严格区分。
- B3 的必须路径已收敛为客户端 batch 接线并完成；server hints 仍可在 flare-im-core 继续增强。
- 迁移最低风险点：Phase A 独立落地即改善热启/重连；不必等 Phase B 服务端。
- **机制/语义边界（本轮定位）**：core 只有 `im.messages`/`im.conversations` 两个内置领域 + `SyncDomain` SPI；好友/群/第三方业务领域一律在 Social/插件/业务侧 `impl SyncDomain` 注册。任何"core 里出现好友/群业务规则"都是设计味道。
- 落地顺序：**A（体感）→ E1–E4（SPI+收编内置领域）→ C（领域无关调度）→ B（多路复用全局流，先降级 B4）→ F（频道读扩散，依赖 B 多路复用）→ E5–E6（注册 SPI + 好友/群参照）→ G（媒体管线，可在 C 后并行）→ D（预算/单测验证）**。**H（三端 computer-use/browser 验收，账号 11~20）在 A/B/C/F/G 每条 sdk 改造落地后增量跑对应场景，非仅末尾一次。**执行走 executing-from-planning / autonomous-executor。
- **自主执行的诚实边界**：本 PLAN 已结构化到可无人值守推进（每步有 verify、每 Phase 有 gate、停机条件明确）。但规模跨三主轴 + 三端 + 跨仓，实际会在 gate 红灯、跨仓/环境里程碑(F4/G6/E6/H)、或需产品决策处**干净停机上报**，而非假装跑完。"告知即全部跑完"以此为限：能自动跑的自动跑到底，该停的地方明确停。
- **范围提示**：本 PLAN 现跨三条主轴（同步收敛 + 领域无关内核 + 频道/媒体）。频道读扩散(F4)、媒体转码/CDN(G6) 的服务端/基础设施部分仍是跨仓/跨栈里程碑；B3 必须路径已通过服务端既有 `MultiConversationSync` + 客户端 batch 接线完成。
- 后续明确边界：若要继续 F4/G6/E6/H1-H8，需要分别在 flare-im-core、infra、flare-social 和真实三端环境中执行；本仓 core-sdk 可自动验证面已绿。

## WASM 重建链（每轮 app 验证前必走，非显然，务必照此）
1. `cd flare-im-core-sdk && cargo xtask build wasm`（wasm-pack 编译 core→wasm，~75s，产物落**中央** `flare-im-core-client-sdk/native/artifacts/wasm/`）。前置：wasm-pack + `wasm32-unknown-unknown` target（已装）。
2. **手动同步**中央→各 app（xtask 只更新中央，不碰 app 副本）：`cp native/artifacts/wasm/* examples/<app>/native/artifacts/wasm/` 及 `examples/<app>/dist/flare-core-wasm/`。
3. 重启该 app 的 vite（清 `node_modules/.vite` 缓存）→ 浏览器硬刷。
4. 首次 WASM 加载慢（~45s，5.6MB + re-optimize），screenshot 可能超时，稍等重试。

## Execution log
- 基线：`cargo test --features storage-sqlite --lib` → **395 passed**（改造前绿）。
- A1 done：StartupClass 分类器 + warm_start(Silent) + engine 接线；**396 passed**。
- A3 partial：reconnect 同步转 Silent（重连不弹 loading）；396 passed。结构性去重待 Phase B。
- A4 done + A2 partial：`SyncNotify::Readiness{LocalReady/ForegroundFresh/Converged/Degraded}` 恒可见；bootstrap 发 LocalReady、orchestrator Init/Bg Finished 映射 ForegroundFresh/Converged；关键发现——event_bus publish 会丢弃 silent-sync，故就绪信号必须恒可见（也印证 A1/A3 静默正确抑制了热启/重连的 sync spinner）。**397 passed**。
- E1 done：`SyncDomain` SPI 骨架落 `kernel/sync/domain.rs` + spi 导出（领域无关内核地基，群/好友/任意业务可 impl 注册）。**400 passed**。
- 本批累计：A1 / A3(部分) / A4 / A2(信号) / E1，核心 395→**400 绿**，纯离线无后端依赖。
- B4/C2 done：`MessagesSyncTask` 改为 attention ordered targets + `MultiConversationSync` 批量 catch-up；删除周期前台 worker；启动 `backfillVisibleHistories` 默认/调用改 false。verify: `sync_protocol_adapter` 6 passed、`sync_task::messages` 3 passed、`client::im_client` 15 passed、旧路径 grep 归零。
- B1/B2/D2/E2/E3 done/partial：新增 `DeviceGlobalSyncPoint(Store)`；`SyncScope::Domain(DomainId)`；`SyncDomainContext`；`GlobalDomainItem` + `route_global_stream(_with_context)`，支持全局缺口报告 + 多领域路由。verify: `kernel::sync::global_cursor` 2 passed、`kernel::sync::task` 6 passed、`kernel::sync::domain` 10 passed、`kernel::sync::domain_registry` 5 passed。
- D1/D3/F/G core slice：`LaneSpec.trigger` 固化；`MediaWarmupPlan`；`ConversationFanoutMode`/`ConversationTimelinePolicy`；`MediaProgressiveState/Event`；`MediaDownloadManifestVo/PartVo`。verify: `media_warmup` 2 passed、`model::conversation` fan-out tests passed、`domain::media_progressive` 2 passed、`domain::download_manifest` 3 passed。
- WASM/app verification：`cargo xtask build wasm` 成功并同步中央 artifact 到 web/tauri/electron；`shasum -a 256 flare_im_core_sdk_wasm_bg.wasm` 中央与 5 个副本一致（hash `4299853c17731836de7b5a7365ec92d91f6485f29449cf3c7489e41a0be2b115`）。
- Final verification：`cargo test --manifest-path flare-im-core-sdk/Cargo.toml --features storage-sqlite --lib` → **431 passed**；`cargo test --features storage-sqlite,lifecycle-sqlite --lib` → **434 passed**；`cargo test --features storage-sqlite,lifecycle-sqlite` → **454 passed, 5 ignored**。
- Packages/app verification：`flare-core-vue-im-ui npm test` → **109 passed**；`vue-tsc` passed；web app `npm test` → **27 passed**；web app `npm run build` passed bundle budget; tauri app `npm run typecheck` passed; tauri app `npm test` → **6 passed**; web Playwright E2E messaging flow → **1 passed**（首次 webServer 超时由 Vite 首次依赖优化/无 ready 输出触发，手动预热后复跑通过）。
- Warm startup calibration done：新增 `SyncTrigger::WarmStartupCalibration` / `SyncReason::WarmStartupCalibration` / `SyncVisibility::NonBlocking`；热启校准与后台补齐保留同一 run context，触发 `sync.started/progress/finished/state/readiness`，但 `is_user_visible=false`，不会弹 blocking loading。
- Warm calibration catch-up done：`sync_conversations_with_context` 对 `WarmStartupCalibration` 启用 startup catch-up 分支；热启不只是本地出图，也会真正做数据校准和补齐。verify: `warm_start_calibration_runs_init_and_background_with_status_events` passed、`warm_start_background_phase_keeps_calibration_status_visible` passed。
- A3 connected reconnect done：ready + same user + transport alive 走 catch-up-only nonblocking run，不 `stop_sync` / disconnect / 重复 recover pending；真实断线仍走 transport reconnect。verify: `reconnect_plan_uses_catch_up_only_when_ready_transport_is_alive` passed、`nonblocking_catch_up_does_not_abort_current_sync_run` passed。
- Startup wait report done：新增 `StartupSyncTiming` / `StartupSyncWaitReport`。确定性报告：冷启动 LocalReady **20ms**、ForegroundFresh **260ms**、Converged **640ms**；热启动 LocalReady **5ms**、ForegroundFresh **70ms**、Converged **180ms**、HotCalibration **175ms**。
- Contract propagation done：新增 `sync.readiness` 事件契约（cCode 4008 / `im://sync_readiness`）并把 sync event run context 字段传播到 bindings、sdk-spec、TypeScript、Swift、Kotlin 生成物；修复 TypeScript adapter 对 optional default 的生成，避免 `mentionUsers` / `mentionAll` 默认值丢失。
- Final verification refresh：`cargo fmt --manifest-path flare-im-core-sdk/Cargo.toml --all` passed；`cargo fmt --manifest-path flare-im-core-sdk/bindings/shared/Cargo.toml --all` passed；`cargo test --manifest-path flare-im-core-sdk/Cargo.toml --features storage-sqlite,lifecycle-sqlite --lib` → **441 passed**；`cargo test --manifest-path flare-im-core-sdk/Cargo.toml --features storage-sqlite,lifecycle-sqlite` → **461 passed, 5 ignored**；`cargo test --manifest-path flare-im-core-sdk/bindings/shared/Cargo.toml` → **39 passed**；`cargo xtask core-codegen-check` passed；`cargo xtask codegen-check` passed；TypeScript SDK `npm test` → **76 passed** + `npm run build` passed；Vue IM UI `npm test` → **109 passed** + `npm run typecheck` passed；web app `npm test -- --reporter=dot` → **27 passed** + `npm run build` passed bundle budget；tauri app `npm run typecheck` passed + `npm test` → **6 passed**；`cargo xtask build wasm` passed and central/Electron WASM hashes match.

### Phase I/J Execution log（2026-07-02 深度优化波）
- I1 done：`apply_events` → 对齐 `Vec<bool>`；`covered_item_seqs`+成功事件 seq 推游标；`sync_page_cursor_stops_before_failed_event_seq`。461→**462**。
- I2 done（client）：`snapshot_cutover_required` + `request_sync_snapshot` + `apply_snapshot_row` + `resync_conversation_from_snapshot`；触发点=消息页 stale/hints + 关键事件缺口。**464**。
- I3 done：`GlobalStreamRouteReport.safe_global_seq`（连续∩应用成功前缀；unrouted 不阻塞）；+3 测试（deferred 阻塞/unrouted 放行/截断无害）。**467**。
- I4 done：`critical_events_provably_clean` 跳过 + 完成后抬检查位点（QueryEvents O(N)→O(变化)）；已读恒 delta 删全量模式。**468**。
- I5 done：`ForegroundPullGuard` + `spawn_foreground_pull`（open_timeline 即时定向补拉）。**469**。
- I6 done（proto+server+client）：proto `SnapshotConversationRow.summary` + `SyncSnapshotSync.{newest_first,conversation_page_limit}`；server 游标容错/页水位游标/消息查询移到分页后/conversations_sync 每会话 1 条/summary 填充；client `snapshot_cursor_watermark_ms` + `cold_start_bundle_sync`。server **20 passed**；sdk **470**。
- I7 done：批量读 trait（默认逐条回退）+ SQLite IN/GROUP BY 覆盖 + `multi_conversation_start_positions` 3 批量调用；`batch_cursor_read_matches_single_reads`。**471**。
- I8 done：`next_batch_plan` 批间重排 + P0 单飞抢占；+2 测试。**472**。
- I9 done：`app_in_background` 接 `set_heartbeat_app_state`；`adaptive_sync_budget`；`yield_for_frame` 挂 multi 页/bundle 行。**473**。
- I10 核实为已存在（dispatcher waterline 三源 + server envelope 水位/纯 ping 双侧有测试）。
- J1 done（server）：`multi_conversation_sync` buffered(8) 保序并发；env_registry 补 16 个未登记 var；gateway `conversation_subscription.rs` 8 处 expect→毒锁恢复。`cargo test --workspace`（flare-im-core）全绿无 FAILED。
- J2 core 侧修复：`ViewApi::close` 关时间线清 attention 前台（否则已关会话永远 P0）。sdk 全量 **473 passed, 5 ignored** 复验。
- J3 done：proto 双侧编译绿，无 bindings 契约变化（未触及 L1 API 面）。
