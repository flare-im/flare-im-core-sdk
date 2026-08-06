# 身份缓存闭环（消息/会话名称头像:写时入缓存,读时批量 join,当前身份语义）

## Goal
进会话/列表时,消息与会话渲染**当前**发送者/用户名称头像,数据来自**已填充的本地身份缓存**(批量 join,读时零逐条组装);缓存由三条写入口喂:① 入站消息自带的 sender_name/avatar 被动 upsert、② sync 用户/群变更、③ 业务端一等 upsert 接口。缓存 miss 时回退消息内嵌的"发送时快照"(sender_name),**名字永不掉成裸 ID**。无逐条写放大。
"done" = web 端登录 12 打开群会话,发送者显示真实昵称(非 ID);新增单测过;`cargo test` SDK 绿;upsert 接口经 core→bindings→packages 暴露。

## Constraints & decisions
- **语义=当前身份**(用户已定):改名/换头像后历史消息显示最新。实现=本地缓存+批量 join;miss 回退消息内嵌 sender_name(as-of-send 快照,proto 已带,见 message.rs:171-176)。
- **不在每条消息行冗余存可变身份**(避免改名 O(消息数) 写放大/膨胀/一致性风险)。消息只保留 proto 自带的 as-of-send 快照(本就免费)+ sender_id。
- 会话摘要**已物化**(display_name/last_message/unread/member_preview),本计划不改其模型,只确保识别信息来源一致。
- 层归属(flare-im-spec):缓存表+批量 join+upsert 抽象 → flare-im-core-sdk(Rust);upsert API → core client api → bindings(uniffi+wasm)→ packages(TS SDK);"何时同步好友/群"=业务侧(不在本计划)。
- 无兼容包袱:一条干净路径。
- 既有事实:UserReader.get/get_many(本会话已批量化)、UserWriter.save_batch(upsert 底座)已在;IndexedDB provider 用 MemoryUserProfileStore 存 profile;client/api 无 profile surface;bindings 无 profile surface。

## Status: IN PROGRESS — Step A 完成并验证;B–E 待办(C/D 是 bindings+packages 大面,建议单独一轮)
Current focus: 决策点——是否继续做 bindings(uniffi+wasm)+packages 暴露(让 web/JS 能调 upsert),这是让 web 可见结果的前提

## 架构考古结论(2026-06-30,重要,改写了原步骤)
系统已 ~80% 建好,缺的是**触发器**,不是基础设施:
- ✅ `ProfileProvider` trait(extension/mod.rs:431)= 业务可注入的**拉取**源,经 builder `add_profile_provider_arc` 注册;但**没有任何代码在读/同步时调用它**(死链)。
- ✅ `apply_user_profile`/`UserProfileProjectionApplier`(projections/user_profile.rs)= 把一个 profile 写进缓存 + patch 会话参与者 + 更新会话展示字段;但**无调用方**。
- ✅ `UserReader.get_many` 批量读 join(本会话已加)。
- ✅ `IMMessage::display_name()` 已回退 `sender_display_name→sender_name→sender_id`(message.rs:616);头像用内嵌 `sender_avatar`。
- ❌ **根因:全链路没有名字数据源**——server 不存/不下发用户展示名(proto sender_name 字段在但 server 不填)、参与者 nickname 测试数据为空。所以名字掉成 ID。**这正印证用户判断:IM core 不拥有用户身份,必须由业务端喂。**
- ❌ 缺口:① 业务 **push** upsert 公共 API 未暴露(core/bindings/packages);② **read-through**:打开时缓存 miss 未触发 `profile_providers.get_profiles`→apply。

## 重排步骤
- [x] ~~1. 读侧 miss 回退~~ 已存在于 `display_name()` 回退链,无需改。
- [x] **A. 公共 push upsert API(core)** ✓:`IMClient::upsert_user_profiles(Vec<UserProfile>)`(im_client.rs:599)→ 既有 `apply_user_profile`(缓存+参与者+会话快照)+ 发 `ConversationEvent::Updated` 刷新受影响会话。验证:新增集成测 `timeline_read_resolves_current_identity_then_falls_back`(命中→Alice、miss→内嵌Bob、都无→u3);SDK 411 测全过。
- [ ] B. read-through(core):打开 timeline 时,缓存 miss 的 sender_ids → `profile_providers.get_profiles(missing)` → `apply_user_profile` → 发视图更新。用 mock ProfileProvider 测。
- [ ] C. bindings 暴露 A(uniffi+wasm,camelCase)。
- [ ] D. packages(TS SDK)暴露 `upsertUserProfiles` + 可注册 ProfileProvider。
- [ ] E. web 验证:在 web 示例喂一个 profile(或注册 provider)→ 打开会话该用户显示喂入名(而非 ID)截图。
- [ ] F. 收尾:PLAN + 记忆 + 清理。

## Phase-1 五端铺开(PLATFORM_COMPLETION_PLAN 一期:web/tauri/iOS/flutter/android)
契约管线已摸清:改 `bindings/contract/*.json` → `cargo xtask core-codegen`(+`typescript-contract`+`platform-contract`)自动生成 dispatch + Swift/Kotlin/Dart/TS 桩。一处契约 → 五端可调。
- [ ] P1. core 前置:`UserProfile` 加 `#[serde(rename_all="camelCase")]`(wire=userId/nickname/avatarUrl;已确认无 snake_case JSON 依赖)。
- [ ] P2. 契约:`direct_invoke.json` 加 `user.upsert_profiles` 路由(result unit,body 取 `profiles` → `client.upsert_user_profiles`);`apis.json` 加 `user` 模块同名条目(core/c/tauri)。
- [ ] P3. 重新生成 + 校验:`core-codegen`、`typescript-contract`、`platform-contract`;`*-check` 或编译验证。
- [ ] P4. 构建桥验证:uniffi + wasm 编译过(wasm 经 xtask build wasm)。
- [ ] P5. TS SDK 暴露 `upsertUserProfiles` 客户端方法(若生成只产类型,补薄封装到 client surface)。
- [ ] P6. 五端 app 接线(都能使用):各端在合适时机调用(如把会话参与者 nickname / 当前用户资料喂入缓存)。web 实测;iOS sim/android compileKotlin/flutter/tauri 各自构建验证(env 支持范围内,逐端如实标注)。
- [ ] P7. 收尾:PLATFORM_COMPLETION_PLAN 勾选 + 记忆 + 截图。

## Phase-1 进度与如实状态(2026-06-30 checkpoint)
- [x] P1 ✓ `UserProfile` 加 camelCase(确认无 snake_case JSON 依赖)。
- [x] P2 ✓ 契约:`direct_invoke.json` 加 `user.upsert_profiles`(body 取 `profiles`→`client.upsert_user_profiles`,路径用公共 `spi::UserProfile`);`apis.json` 加 `user` 模块。
- [x] P3 ✓ `cargo xtask core-codegen` 重新生成 dispatch/contract(shared/uniffi/wasm/c/tauri 均含新路由)。**`core-codegen-check` EXIT=0(绿)**。
  - ⚠️ 排障记:首次 codegen 后单独 `cargo build bindings/uniffi` 污染了共享 target 的 core 制品,导致 xtask 报 33 个 JsonSchema 错(假象);`cargo clean -p flare-im-core-sdk` 后恢复。教训:别在同一 target 里穿插单 crate 构建后立刻跑 xtask。
- [x] P4 ✓ uniffi binding **编译通过**(dispatch 调用 `client.upsert_user_profiles` 成立)。wasm 待编译验证。
- [x] **P5 ✓ 完成**:`sdk-spec/modules/user.json`(transport `contract-invoke-json`、cApi `flare_sdk_invoke_json`、request `UpsertUserProfilesRequest`=FlareJsonObject 别名、response Unit)+ 注册进 `sdk-spec/manifest.json`;从 apis.json **移除** user 模块(JSON-dispatch 路由不该在 apis.json,对照 sync.conversation_summaries)。重生 expanded-spec/typescript-contract/platform-contract/platform-adapter。**`core-contract` 绿、`core-codegen-check` 绿**。生成产物:TS `api/modules/user.ts`+`bridge_contract.userUpsertProfiles`+`UpsertUserProfilesRequest`;Swift/Kotlin/Dart bridge contract 均含。
- [x] **P6(web)✓ 打通+实测**:手写 `DefaultUserApi.ts`(invokeVoid→`userUpsertProfiles`)+ `defaultFlareImClient` 接 `client.user`;vue-im-ui composable 加 `upsertUserProfiles(profiles)`(+DEV window 钩子)。TS `tsc --noEmit` 0 错。**Playwright 实测 account 12**:发送者"14"→喂入后再开会话显示"✅身份缓存生效",未喂的"17"仍回退 id。全链 TS→wasm `flare_sdk_invoke_json`→dispatch `user.upsert_profiles`→core→缓存→批量 join 读→渲染,验证通过。
- [x] **P6(native)✓ SDK 层全部接线+编译过**:`platform-api` 生成各端 UserApi 接口 + client 协议含 `user`;但 **adapter 实现 + client 装配是手维护**(生成器对新模块不产 DefaultXxxApi/不接 client——**生成器缺口**,已记)。手写:Swift `DefaultUserApi.swift`+接 `DefaultFlareImClient.swift`;Kotlin `DefaultUserApi.kt`+接 client;Dart 内联 `_DefaultUserApi`+接 client。**验证全绿**:Swift `swift build` ok、Kotlin `:flare-core-android-sdk:compileDebugKotlin` BUILD SUCCESSFUL、Dart `dart analyze` no issues、TS `tsc` 0 错。Tauri 复用 TS SDK + 共享 vue-im-ui,同 web 路径。
  - 剩余"完善"= 各 native **示例 app** 在自身流程/UI 里实际调用 `client.user.upsertUserProfiles`(Swift/Kotlin/Dart app 代码),属逐 app UI 增量;SDK 能力已全端就绪。
- [x] **P7 ✓ 五端示例 app 全部接线 + 构建验证**:
  - Web:vue-im-ui composable `upsertUserProfiles`(已 E2E)。
  - Flutter app:`flare_core_sdk_wrapper.upsertUserProfiles(profiles)` → `dart analyze` no issues。
  - Android app:`SdkLabViewModel.runUpsertUserProfiles()`(probe `user.upsertUserProfiles`)→ `:app:compileDebugKotlin` BUILD SUCCESSFUL。
  - iOS app:`SdkLabViewModel` 加 `user.upsert_profiles` case → `swift build` ok。
  - Tauri:复用 TS SDK + 共享 vue-im-ui,同 web。
  - **结论:PLATFORM_COMPLETION_PLAN 一期五端(web/tauri/iOS/flutter/android)全部"能使用"身份缓存写入口,SDK+app 均构建通过,web 端到端实测;无名字数据源时回退发送时快照/ID,业务喂入后即显示当前身份。**
- [ ] ~~P5 占位~~:**sdk-spec 模块层**才是各端 typed 方法的真正生成源。需:① 定义请求 DTO(`UpsertUserProfilesRequest{profiles:Vec<UserProfile>}`,带 JsonSchema/Serialize/camelCase;注意 presence 那些 `*Request` 类型的来源是 schema 合成,需查清注册点);② 新增 `sdk-spec/modules/user.json`(镜像 presence:method `upsertUserProfiles`→op `user.upsert_profiles`,request DTO,response Unit,cApi `flare_sdk_upsert_user_profiles`);③ `expanded-spec`/`typescript-contract`/`platform-contract` 重新生成 → 产 TS `SdkOperation`/`bridge_contract`/`api/modules/user.ts` + Swift/Kotlin/Dart 桩。**当前 `core-contract` 因缺此层对 `user.upsert_profiles` 报 parity 错(唯一新红;`spec` 另有与本改无关的既存红:BuildLocationMessageRequest.latitude Double)。**
- [ ] P6 五端接线(P5 完成后才有 typed 方法可调):web 实测;iOS/android/flutter/tauri 各自构建。
- [ ] P7 收尾。

## 重要发现 / 当前状态(给下一轮)
- 仓库**进入本轮前已 dirty**(大量 bindings/examples/Cargo.toml 未提交改动)且 `spec` 检查**本就红**(location Double)。故未做大范围 `git checkout`(会毁掉既存未提交工作)。
- 已交付且验证:core `upsert_user_profiles`+测(411 绿)、bindings/contract 路由(dispatch 正确、uniffi 编译、codegen-check 绿)。
- 真正"让各端能用"的门槛 = P5 的 sdk-spec 模块+DTO 层(生成各端 typed 方法),然后逐端接线+构建。这是独立的、应专门一轮做的工作量(一个 typed API 跨 4 个 codegen 层 + 5 app + 5 构建)。
- 应用侧也可走通用 dispatch(`invoke` operation `user.upsert_profiles` + `{profiles}`)先用起来,但 TS `SdkOperation` 联合类型尚未含该 op(同样由 P5 补)。

## Steps
- [ ] **1. 读侧 miss 回退**:`view_assembler` 的 `fill_sender_profile`/`fill_sender_profiles_for_messages` 在缓存 miss 时,用消息内嵌 `sender_name`/`sender_avatar`(proto 自带)填充 display_name/avatar,保证名字不掉成 ID。加单测(命中缓存→用缓存;miss→用内嵌快照)。
- [ ] 2. 被动population:入站消息落库路径(dispatcher/save_batch 前后)把 `{sender_id→sender_name,sender_avatar}` 去重后 `user_writer.save_batch` 进缓存。仅当非空且与缓存不同才写(避免无谓写)。加单测。
- [ ] 3. 业务端 upsert API(core):`client/api` 加 `upsert_user_profiles(Vec<UserProfile>)`(底层 UserWriter.save_batch)。供业务同步好友/用户时调用。加单测。
  - [ ] 3b. (可选,本轮评估)群信息 upsert:先只做 user profile;群名片/群成员昵称作为 phase 2 记入 Notes。
- [ ] 4. bindings 暴露:uniffi + wasm 暴露 `upsert_user_profiles`(camelCase JSON 契约)。
- [ ] 5. packages(TS SDK)暴露:web/native adapter + sdk-spec 暴露 `upsertUserProfiles`。
- [ ] 6. 验证:`cargo test`(core)绿;rebuild wasm + web 端登录 12 实测发送者显示真实昵称(非 ID)截图;vue-im-ui smoke 绿。
- [ ] 7. 收尾:更新本 PLAN + 记忆;清理临时脚本。

## Notes / open questions
- web 重建命令:`cargo xtask build wasm`(产物落 bindings/wasm/pkg,dev vite :1430 直供);dev server 当前后台运行中(brpe6j1vb)。
- 群成员昵称(群名片)= phase 2:通常按会话维度的成员缓存或 as-of-send 快照,不混入 user profile。
- version 失效:UserProfile 暂无 version 字段;当前身份变更后"已打开视图重算"先靠下次打开/事件刷新覆盖;若需要立即重算再评估加 version + 视图失效信号(记 phase 2,避免过度设计)。
- 校验点:Step 2 的被动 population 不要在热路径(投递)上加阻塞写——用非阻塞/批量,符合 smoothness 预算。
- 入口文件:read 侧 src/application/usecases/message/view_assembler.rs;UserReader/Writer src/domain/repository/user_repository.rs;client api src/client/api/;bindings bindings/{uniffi,wasm}/src。
