# 冷/热启动增量补齐：时间×字节全栈审计（Phase Q）

## Goal
冷启动与热启动在"最少时间 + 最少网络字节"下达到数据稳定 0 丢失：
1. 量化并压缩启动关键路径的 **RPC 往返数** 与 **传输字节**（零增量热启动应 ≈1 个空响应 RPC；冷启动首屏应 1 RPC 出图）。
2. 找到并修复剩余的串行化/放大点（服务端逐会话查询、客户端串行分页等）。
3. 全链验证（单测 + web E2E 回归）后对改动面跑 /simplify。

## Constraints & decisions
- 前序 Phase A-P 已覆盖大量优化（冷启 bundle/列表增量存储级过滤/changed-only 跳过/水位游标/页缓存/批量读/防熵探测）——本波只找**剩余**缺口，勿重做已验证项（dispatcher CONC-1 勿再审）。
- flare-im-spec：正确性 > 流畅 > 性能；一条路径原则；层次归位。
- 审计镜头：①热启动零增量 RPC 数 ②冷启动每页服务端查询形状（逐会话 vs 单窗口查询）③分页流水线化 ④传输压缩（flare-core 层）。
- 验证板：core-sdk cargo test / orchestrator+conversation+storage tests / web Playwright 3 specs（后端运行中）。

## Status: DONE（Q0-Q16 全绿，2026-07-03）
Current focus: —

## Steps
- [x] **Q0 审计完成** — 发现：
  ①热启动零增量已最优：sync_foreground_convergence = 列表增量 1 RPC（空响应）+ critical events 全跳（本地水位判定 0 RPC）+ messages changed-only 0 RPC + 读状态 delta 0 push。无需改。
  ②**冷启动放大 A（高）**：`sync_critical_events` 冷启时全部会话 critical 游标=0 → `watermark_provably_clean(max_seq,0)`=false → **N 个 QueryEvents RPC**（500 会话=500 个空响应 RPC）。修法：bundle `apply_snapshot_row` 落地时同步建立 `critical_event:{cid}` 检查位点=last_seq——存储服务的消息是"当前态物化"（撤回/编辑/反应已反映在返回行里），≤last_seq 的事件要么已体现在首页消息、要么指向不在本地的历史，位点前移无损。
  ③**冷启动放大 B（中）**：`get_sync_snapshot` 每页 50 会话 → 50 个逐会话 QueryMessagesBySeq 存储 RPC（buffered 8 ≈ 7 波）。修法：新增存储批量窗口 RPC（`ROW_NUMBER() OVER (PARTITION BY conversation_id ORDER BY seq DESC) <= limit`，投影原样复用），每页 1 RPC 1 SQL。
  ④**冷启动串行（中）**：客户端 bundle 取页→应用→再取下页全串行。修法：预取流水线（应用第 N 页时并发请求第 N+1 页）。
  ⑤字节层压缩属 flare-core 跨仓里程碑，本波不动（protobuf 已紧凑；记录不做）。
- [x] **Q1 冷启 critical 位点随 bundle 建立**（放大 A）——经 /simplify altitude 审查下沉为 `establish_snapshot_cursors`（消息游标+关键位点双位点同源），bundle 与 `resync_conversation_from_snapshot` 共用；顺带修掉消息路径 gap-too-large cutover 不抬关键位点的不一致 + 删掉 sync_critical_events 里的位点特例分支。验证：不变量单测既有 + 全量套件 + 真实 E2E。
- [x] **Q2 客户端 bundle 页间流水线** —— `futures::join!(应用第N页, 请求第N+1页)`；/simplify 后按值消费整页（summary take + 消息 move 进收敛管线，免 50×30 深拷贝）+ 让帧改为每 8 行一次（wasm setTimeout(0) 嵌套钳制每页最多白等 ~200ms）。
- [x] **Q3 存储批量窗口查询**（放大 B）——落地后经 /simplify 两轮升级：①efficiency 审查发现初版 `ROW_NUMBER 全表窗口` 会扫全历史+逐扫描行算昂贵投影 → 改 **unnest+LATERAL**（每会话走 (tenant,cid,seq) 索引只读 limit 行，投影只对返回行求值）；②altitude 审查推动 proto 泛化为 `targets{conversation_id,after_seq}+newest_window`（tail/增量两形态各自最优 SQL），**multi_conversation_sync 成为第二调用方**（重连 catch-up 消息页 N 次存储 RPC → 1 次批量）。投影抽取 `MESSAGE_ROW_PROJECTION_WITH_VISIBILITY` 单源共享（消除既有双份内联复制）；pin 谓词合并 `apply_pin_flags` 单实现；trait 无默认实现（批量是性能契约，mock 显式实现语义可见）。
- [x] **Q4 /simplify（4 并行审查代理：reuse/simplification/efficiency/altitude）+ 全量验证** —— 已应用：LATERAL 重写、proto 泛化+multi 接入、投影/pin 单源、trait 去默认、index_by_id 单遍、bundle 按值消费+累加块去重、让帧分块、ack_keys zip 复用、`sync_visibility_floor` 领域助手提取（8 文件 10 站点收敛）。**验证**：core-sdk 416/557、reader 11、orchestrator 23、flare-im-core workspace 438 全绿；后端全新重建后 **web E2E 4/4 passed (16.2s)**（含新设备冷启动用例=批量窗口真实链路，热启动 394ms），sync-orchestrator/storage-reader/conversation 日志 0 ERROR。
- [x] **Q5 客户端批量游标写** — `SyncCursorWriter::save_conversation_cursors`（默认逐条=今日语义，SQLite 单事务覆盖；IndexedDB 的持久化本就 spawn 非阻塞不需覆盖）+ usecase `save_watermarks` 批量本地写；bundle 页改「页内落数据、页末一批建双位点 + 统一入去抖远端队列」——每页 ~100 次串行 upsert → 1 事务（I1 先数据后游标仍成立，只是更晚）。
- [x] **Q6 存储窗口端点按值转换** — contracts 新增 `message_into_proto`（move 内容字节/字符串/map），窗口端点整页免深拷贝。
- [x] **Q7 远端游标冲刷 worker** — read_seq 从逐条点查改 `get_many` 批量预取（RPC 侧受 PacketSender 串行等待约束，本地读是可拿的部分）；`update_remote_cursor_seq` 拆出 `push_remote_cursor(cid, seq, read_seq)`。
- [x] **Q8 tail-page 谓词单源** — `is_backfill_tail_page(after, before)` 定义在 reader domain（分页约定唯一实现），Postgres store 与 gRPC handler 共用。
- [x] **Q9 ensure_conversation 约定收敛** — `flare_grpc_proto::ensure_conversation_request`（attributes 携带 conversation_id 的建群约定唯一实现），ingest 管线与 sync 编排两处 proto 构造收敛（conversation 服务消费端用领域类型属不同层，仅共享 id 约定，保持独立）。
- Q5-Q9 验证：core-sdk 416/557、flare-im-core workspace **438** 全绿；后端再重建后 **web E2E 4/4 (14.3s)**，热启动 346ms，关键服务日志 0 ERROR。
- [x] **Q10 LATERAL 索引地基实测证实** — `idx_messages_tenant_conv_seq(tenant_id, conversation_id, seq)` 存在于 deploy/init.sql；对真实库（TimescaleDB hypertable）EXPLAIN：Nested Loop + 每会话 `Index Scan Backward` + Limit（分块 Merge Append 仍索引反扫）——批量窗口的性能假设被真实计划证实，零改动收口。
- [x] **Q11 WASM 体积 -36%** — 审计发现 `wasm-opt = false` 且 wasm 构建用默认 opt-level=3；开启 binaryen `-Oz`（+bulk-memory/nontrapping-fp）+ xtask 内固化 `CARGO_PROFILE_RELEASE_OPT_LEVEL=z`（仅作用 wasm 构建，不动 native release）→ **5.0MB → 3.18MB raw（-36%），gzip 1.5 → 1.18MB（-21%）**；web 冷启网络传输与 V8 编译时间随字节数近线性下降。verify: 优化产物重部署后 E2E 4/4。
- [x] **Q12 发送热路径克隆消除** — `SendMessageCommand::execute/execute_via_queue` 由 `&self` 改消费 `self`：发送入队不再整体克隆 IMMessage（媒体消息 encoded_content KB 级）；同时确认发送→本地回显路径不等网络（enqueue=actor 本地落库即应答，符合 <16ms 预算的结构前提）。verify: 416/557 全绿 + E2E 4/4 (15.2s)。
- [x] **Q13 gateway 群扇出分组编码** — 审计：内层 proto 单次编码 ✓、Arc 单帧共享 ✓，但外层 wire 帧（序列化+压缩）逐连接重复（每连接自协商格式的设计代价）。落地：flare-core `fanout_frame_grouped_by_encoding`（无加密连接按（格式,压缩）分组每组序列化一次共享字节+保留 per-connection 授权门；加密连接回退逐连接——密文可能绑定连接态）→ `ConnectionManagerTrait::send_frame_to_connections`（trait 默认逐连接）→ `ServerHandle::send_to_connections` → gateway push_port 删自有 64 并发循环改单调用。N 订阅者下行 serialize+compress N 次 → 组数次（通常 1）。verify: flare-core 159 / gateway 48 / workspace 438 全绿；重建后 E2E 4/4。
- [x] **Q14 web 消息列表虚拟化审计** — 已达标零改动：MessageList.vue 有虚拟窗口（top/bottom spacer + virtualOffsets + viewport 追踪），满足 spec O(visible) 预算。
- [x] **Q15 electron 热启动白拿** — electron 与 web/tauri 共享 vue-im-ui + 同款 router，补同款 resume 守卫 6 行即获完整热启动；vue-tsc 绿。
- [x] **Q16 冷启动再压缩（用户点名）** — 基线实测 login→列表 1588ms（web 本地环回，小账号）。三个实验，两落地一否决：
  ①**WASM 预热落地**：vue-im-ui 组合根挂载即后台 `getSdkVersion()`（无会话直连操作）触发 3.2MB 运行时加载——真实用户在登录屏输入期间完成下载/编译，登录点击不再背 wasm 冷加载（E2E 自动化秒填秒点测不出，真实用户收益明确、零风险）。
  ②**冷分支等编排首页（race+兜底）被测量否决**：等待窗口 2s 内 bundle 首页未落地 → 白等+兜底 = 1588→3756ms 反劣化，已回滚。教训：**没有实测支撑的"理论更优"路径先量再上**。
  ③**bundle 首页放行落地（结构性）**：`cold_start_bundle_sync` 拆为首页内联应用即返回 + 余页后台续拉（self_weak Arc 装配 + 单飞防重 + 每页会话切换守卫 + 未装配/在飞回退内联全量）。大账号首屏从 O(页数) 串行 RPC → **1 RPC**；数据纪律不变（每页应用即建双位点；列表水位仅全部页完成后保存，中断→下次全量兜底不丢）。verify: 416/557 全绿；E2E 4/4（小账号 1524ms 与基线持平——小账号本就 1 页，收益在大账号页数上）。
- 记录不做（已裁决）：单会话起始位点复用批量助手（计算内核已单源，剩余是两路不同 I/O 编排+remote_cursor 语义）；moka 换手写缓存（Phase M 已裁决纯偏好）；persist_converged_messages 每行 cleared-floor 点查预取（需给 apply_snapshot_row 加存储细节参数，收益已被批量写吸收大半）；远端游标批量 RPC（proto 单游标形态，且 PacketSender 串行等待下并发无益——本波已批量化其本地读半段）。

## Notes / open questions
-
