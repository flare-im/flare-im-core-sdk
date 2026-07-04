# 网络韧性五端接线 + Android 秒启动补齐（Phase P）

## Goal
五端（web/tauri/flutter/iOS/Android）达到飞书/TG 级网络体验与启动体验，可检验态：
1. 每端自动喂两个原始信号给 core：平台网络变化 → `connection.notifyNetworkChange`（core 主动重连，不等心跳超时）；前后台切换 → `sdk.setHeartbeatAppState`（core 前台立即收敛+后台降配）。
2. Android 端补齐至 Phase O 水位：kotlin SDK parity 三连（DefaultSyncApi 两方法 + codec + READINESS 分支）+ app 热启动五件套（SavedSessionStore/prepare拆分/resume/入口接线/登录持久化）。
3. 验证：web Playwright 断网→恢复自动重连收发 E2E 实测；flutter analyze+test、iOS swift build+test、android compileDebugKotlin+test 全绿。

## Constraints & decisions
- flare-im-spec：平台只喂**原始信号**，全部策略（去抖/合并/重连/收敛/降配）留在 core（已具备，勿在平台层重实现）。
- **F3 裁决：last-session identity 不下沉 core**——transport prefs/token 重签本就归 app，下沉需新增 contract op + wasm 平台 KV port 而不减少 app 代码；共有策略已全在 core。勿再翻案。
- 网络监听接线放置：vue-im-ui（packages 共享层，覆盖 web+tauri）；flutter/iOS/android 放各 app 的 Core/session 层（无跨生态共享包可放；SDK packages 是生成面不放手写 glue）。
- flutter 前后台已接（event_to_store.dart didChangeAppLifecycleState）；只补 connectivity。
- web 离线检测：`window online/offline` 事件（Playwright `context.setOffline` 可驱动）。iOS：NWPathMonitor + scenePhase。Android：ConnectivityManager.NetworkCallback + ProcessLifecycleOwner（查依赖，若无 lifecycle-process 则用 Activity 生命周期）。
- Android SDK 修复对齐 flutter/apple 同款改法（本 PLAN 姊妹波 Phase O3/O4 已验证的模式）；生成文件按既有手写-对齐-生成风格补（与 flutter/apple 一致）。
- Android app 无 committed gradlew：复用 flutter app wrapper + local.properties + JDK17（memory android-app-build-setup）；快速验证 `:app:compileDebugKotlin`。
- 后端已运行（60051/50050/60084/60060）；vite 1430 需要时重启。

## Status: DONE（P1-P7 全绿，2026-07-03）
Current focus: —（web 全套 E2E 3 passed 17.7s 复归；tauri vue-tsc 绿）

## Steps
- [x] **P1 vue-im-ui 网络+可见性自动接线（web+tauri）** — `startPlatformSignalBridge()`：online/offline→notifyNetworkChange、visibilitychange→setHeartbeatAppState；登录/恢复成功后启动，logout/unmount 停止；SSR 安全。verify: vue-tsc 绿 + vitest 109 passed。落地：useFlareCoreClient `startPlatformSignalBridge/stop`，login+resume 启动、logout+unmount 停止。
- [x] **P2 web E2E：断网→恢复自动重连收发** — `network-resilience.spec.ts`：双用户互发→A `setOffline(true)`→发消息进 pending/失败态→`setOffline(false)`→自动重连→消息补投或重发送达 B；B 全程在线消息 0 丢失。**1 passed (12.8s)**：调试实测 offline→`network_change reconnected=false`、online→`reconnected=true` 主动重连、离线消息 pending 队列自动补投（无需手动重发）、连接回 ready；测试兜底保留重发菜单路径。
- [x] **P3 android SDK parity 三连** — DefaultSyncApi 补 `bootstrapStartupHome`/`backfillConversationHistory` + WireCodec 四函数（startupHomeSyncRequestToMap/ResponseFromJson、conversationHistoryBackfill 两个）+ DefaultEventsApi `READINESS -> Unit`。verify: P5 的 compileDebugKotlin 覆盖（SDK 是 app 的 included build）。
- [x] **P4 android app 热启动五件套** — SavedSessionStore(SharedPreferences) + AppSession 拆 `resumeLocal`(prepare)/`connectInBackground` + FlareAppStore/RootViewModel resume 编排 + 入口(FlareApp/MainActivity)启动尝试 + login 成功持久化/logout 清除。目标形态：FlareApp/入口 resume 门控（resume 尝试中不闪登录页）。
- [x] **P5 android 网络+前后台接线 + 构建验证** — PlatformSignalBridge（ConnectivityManager.NetworkCallback onAvailable/onLost/onCapabilitiesChanged→notifyNetworkChange + ActivityLifecycleCallbacks(MainActivity onStart/onStop)→setHeartbeatAppState）挂登录/恢复后启动、logout 停止。verify: `:app:compileDebugKotlin` + `:app:testDebugUnitTest` **BUILD SUCCESSFUL**。落地：FlareApp Compose DisposableEffect 接 ConnectivityManager.registerDefaultNetworkCallback + LocalLifecycleOwner ON_START/ON_STOP；FlareAppStore.notifyNetworkChange/setAppForeground 转发；resume 门控 LaunchedEffect。
- [x] **P6 flutter connectivity + iOS NWPathMonitor 接线** — flutter：`connectivity_plus` → wrapper `notifyNetworkChange`（event_to_store 桥内与 lifecycle 并列，interface 映射 wifi/cellular/ethernet/none）；iOS：AppSession `startPlatformSignalBridge()`（NWPathMonitor status/interface→notifyNetworkChange + UIApplication 前后台通知→setHeartbeatAppState，macOS 用 NSApplication 通知），start/resumeLocal 后启动、logout/dispose 停止。verify: flutter analyze 0 issues + **62/62 tests** + macos debug build ✓；iOS swift build + **43 tests 0 failures** + iPhone 17 Pro sim **BUILD SUCCEEDED** ✓。落地：flutter connectivity_plus→event_to_store 桥（与 lifecycle 并列）；iOS AppSession startPlatformSignalBridge（NWPathMonitor + UIApplication/NSApplication 前后台通知），start/resumeLocal 启动、logout/dispose 停止。
- [x] **P7 收尾** — PLAN/memory 更新；姊妹 PLAN Phase O 交叉引用本波（Phase P 行加入 status 头）。

## Notes / open questions
- core 侧零改动预期：notify_network_change/set_heartbeat_app_state/退避重连/防熵探测全部已具备并有测试。若接线中发现 core 缺口（如 wasm 侧 connection.notify_network_change 路由缺失）→ 按契约传播序修。
- wasm 侧需确认 `connection.notify_network_change` 在 production runtime 有路由（direct_invoke 列表）——P1 第一步先验证。
- Playwright setOffline 只切浏览器网络栈，ws 断开表现为 socket error → core Disconnected → 退避重连循环；恢复后 window online 事件驱动 notifyNetworkChange 主动重连，两条路径都被测到。
- android ProcessLifecycleOwner 需要 `androidx.lifecycle:lifecycle-process` 依赖；若 app 未引入，改用 MainActivity onStart/onStop 转发（单 Activity app 足够，零新依赖）。
- 断网发送的消息进 reliable queue，重连 `recover_pending_for_current_user` 自动补投（core 已有）；E2E 里两条路径（自动补投 / UI 重发）任一送达即 0 丢失。
