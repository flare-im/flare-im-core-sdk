# 同步收敛架构：本地优先三层收敛 + 设备全局同步点

本文描述 `flare-im-core-sdk` 的数据同步模型——冷启动、热启动、断线重连三条路径如何收敛到一致状态，以及为什么这样分层。

## 设计目标

同步的核心矛盾是：**用户注意力是局部的，数据是全局的**。旧模型对所有会话一视同仁地扁平轮询，代价随会话数线性增长，而用户当下只看一个会话。新模型把"补全什么"和"什么时候补"交给注意力信号驱动。

需要守住的预算：

| 场景 | 预算 |
|---|---|
| 热启动首屏 | < 200ms（纯本地读，不 await 传输） |
| 冷启动主线程 | < 500ms（只 gate 首屏，其余静默回填） |
| 发送回显 | < 16ms |
| 重连 | 不阻塞、不全量重取 |
| catch-up 期间渲染 | 60fps，内存有界 |

优先级次序：**正确性 > 流畅 > 性能 > 及时性**。注意力信号只是 hint——视口判断错了只影响补全顺序，缺口修复机制保证最终完整性。

## 三层收敛

同步分三层，各层的 gate 条件不同：

- **T0 本地水合**：会话列表与上次活跃会话的时间线从本地存储直接出图，不等待任何网络。热启动首屏由这一层负责。
- **T1 首屏 gate**：本地为空时（冷启动），只 gate "top-N 会话摘要 + 前台会话消息"，UI 先出骨架。
- **T2 静默回填**：其余数据在后台按优先级补全，不阻塞交互。

热启动直接走 T0；冷启动走 T1 → T2；重连进入 `CatchingUp` 静默态，只做增量。

## 设备全局同步点

旧模型的游标是 per-conversation 的 `{conversation_id, last_seq}`，导致追赶成本 O(会话数)——每个会话一次同步请求，即 catch-up 的 N+1 问题。

新模型引入**设备级全局游标**：一次 `SyncSince(globalSeq)` 流式请求拿回所有变化，成本降为 O(增量)。已读位点上报同理，从"全量会话逐个 ack"改为只上报本地已读位点超前于已 ack 位点的会话。

per-domain 子游标挂在全局同步点之下——因为并非所有领域都有 `conversation_id`（好友列表就没有）。

> 服务端 `SyncSince` endpoint 未就绪时，降级路径是"先 diff 出变化的会话，再逐会话 catch-up"，语义等价、成本较高。

## 注意力优先级调度

收敛顺序按注意力排序：**前台会话 > 可见列表窗口 > 近期活跃 > 其余**。

调度器是事件驱动的，取代了此前的扁平轮询加周期性前台轮询。历史回补（backfill）过去是独立的启动例程，会和同步抢带宽，现在作为一条低优先级 lane 并入同一调度器，受同一套背压和并发预算约束。

媒体也纳入注意力域：前台会话最近消息的缩略图和 blurhash 随注意力预热，全程脱离渲染帧，内容寻址去重，缓存有界。

重连是高频事件——WiFi/蜂窝切换、进入前台都会触发（带 coalescing），所以轻量路径是硬要求而非优化项。

## 领域无关的收敛内核

core 定位为**通用同步内核 + `SyncDomain` SPI**。消息和会话（`im.messages` / `im.conversations`）只是内置的头两个领域；群、好友以及任意业务对象注册为新领域后，即可复用冷启/热启/重连/追赶/优先级/缺口修复的全套机制。

这要求打开三处原本对"消息"形状的硬耦合：

1. **applier 路由**：事件应用器改为按 `domain_id` 路由的注册表，`SyncEventApplier` 降级为 `im.messages` 的内置 applier，不再是唯一入口。
2. **游标抽象**：`SyncCursorVo{conversation_id, last_seq}` 抽象为 per-domain 子游标。
3. **开放 scope**：`SyncScope` 闭枚举改为开放的 `DomainId`（稳定字符串）。

边界纪律：**内核只提供机制，业务语义留在 Social 或插件**。`social.friends` / `social.groups` 由 Social 侧 `impl SyncDomain` 注册，core 不认识它们的业务规则。

## 冲突收敛与重放

多端收敛复用既有组件，不重造：

- **`SyncEventApplier`**（`usecases/sync/event_applier.rs`）——recall / edit / reaction / delete / read / retention / pin / mark / custom / conversation 全事件按 `operation_seq` 做 LWW 冲突收敛；`missing_target_is_already_covered` 处理乱序与目标缺失时的可重放语义。全局流应用器必须走它。
- **`IncomingMessageConverger`**——身份规范化（单聊 canonical id）、本机 pending 合并回 ack、服务端重复消息去重与状态刷新。多端 echo 收敛在此完成。
- **`seq_repair.rs`**——实时缺口的退避补拉，配有界串行持久化 worker。

显示顺序以服务端 `conversationSeq` 为准；本地乐观项先占位，ack 时校正。

## 分层归属

所有产品中立的同步行为下沉到 `flare-im-core-sdk`（Rust）。注意力信号 API 与就绪状态事件按 `core → bindings → sdk-spec → packages` 的顺序传播。示例应用只负责上报视口和渲染状态。

## 演进方向

- **频道 / 广播读扩散**：大群与频道走 fan-out-on-read 共享时间线，小群保持 fan-out-on-write。读扩散主体在服务端；SDK 侧只做 fan-out 模式决策、共享时间线消费、订阅退订和成员聚合（不枚举成员）。频道是 `im.messages` 领域的一个 sync-strategy 属性（按 `ConversationType::{Channel, Broadcast}`），不是新领域。
- **媒体管线**：core 负责缓存驱逐策略、上下行续传状态机、渐进状态模型；packages 负责平台传输/解码/缩略图适配；转码、CDN、签名 URL 属服务端与基础设施。上行续传清单（`domain/upload_manifest.rs`）与有界内容缓存已就位，下行续传待补。
- **多端草稿与设置**：draft 走 `conversation_settings` patch，未来作为独立的 SyncTask lane。
