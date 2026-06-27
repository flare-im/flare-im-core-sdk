# 同步模块 (sync)

同步引擎负责：**可注册任务的统一编排** + **协议响应处理**。登录后由 `SdkEngine::bootstrap` 调用 `sync_manager.run_sync(user_id)`，编排器按 **Init 阶段内并行** → SyncFinished(Init) → **Background 阶段内并行** → SyncFinished(Background) 执行；会话列表、单会话消息等均由 **SyncTask** 形式注册（如 `ConversationsSyncTask`、`MessagesSyncTask`），统一格式、无额外入口。

---

## 核心类型

| 类型 | 说明 |
|------|------|
| `SyncTask` | 任务 trait：`id()`、`mode()`、`weight()`、`execute(ctx)` |
| `SyncContext` | 执行时注入：`user_id`、`task_id`、`store`、`progress`、检查点读写 |
| `SyncMode` | `Init`（阻塞 UI 就绪）/ `Background`（后台执行） |
| `SyncTaskResult` | 任务返回值：`success`、`message`、可选 `cursor`（检查点） |
| `SyncManager` | 持有已注册任务，提供 `register_task_arc`、`run_sync(user_id)` |

---

## 自定义任务

1. **实现 `SyncTask`**  
   - `id()`：唯一标识，用于进度事件与检查点 key。  
   - `mode()`：`SyncMode::Init` 或 `SyncMode::Background`。  
   - `weight()`：权重，用于汇总进度（total_weight / completed_weight）。  
   - `execute(ctx)`：异步执行，可访问 `ctx.store`、`ctx.report_progress(detail)`、`ctx.load_checkpoint()` / `ctx.save_checkpoint(cursor)`。会话/消息/已读等协议由任务在**构造时**注入处理器（如 `Arc<SyncHandler>`），在 execute 内直接调用，与内置任务一致。

2. **注册任务**  
   - 构建时：`IMClient::builder().add_sync_task(MyTask).add_sync_task_arc(arc_task)`。  
   - 建连后：`engine.sync_manager().register_task_arc(arc_task)`。

3. **执行**  
   仅注册即可，**不主动调用执行**。`bootstrap` 内只调用 `sync_manager.run_sync(user_id)`，由编排器按 Init → Background 顺序执行全部任务；会话列表拉取由默认注册的 `ConversationsSyncTask` 在 Init 阶段完成。

### 示例

```rust
use flare_im_core_sdk::core::sync::{SyncTask, SyncContext, SyncMode, SyncTaskResult, SyncResult};

struct ContactSyncTask;

impl SyncTask for ContactSyncTask {
    fn id(&self) -> &'static str { "contact" }
    fn mode(&self) -> SyncMode { SyncMode::Init }
    fn weight(&self) -> u32 { 10 }

    fn execute(
        &self,
        ctx: SyncContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SyncResult<SyncTaskResult>> + Send>> {
        Box::pin(async move {
            ctx.report_progress("fetching contacts");
            // 拉取并写入 ctx.store，可选 ctx.load_checkpoint() / save_checkpoint(cursor)
            Ok(SyncTaskResult::ok())
        })
    }
}

// 构建时注册
let client = IMClient::builder()
    .config(config)
    .stores(stores)
    .add_sync_task(ContactSyncTask)
    .build();
```

---

## 进度与事件

编排器会发布以下事件（通过 `EventBus::subscribe()` 或 `on_any` 订阅）：

| 事件 | 说明 |
|------|------|
| `SyncStarted` | 开始执行任务列表 |
| `SyncProgress { task, progress, detail }` | 进度更新（progress 0.0～1.0，基于权重） |
| `SyncFinished { phase }` | 阶段结束（`phase`: `Init` / `Background`） |
| `SyncFailed { task, message }` | 某任务执行失败 |
| `SyncTaskCompleted { task }` | 单个任务完成 |

UI 可根据 `SyncFinished(Init)` 判断「初始化同步完成、可展示主界面」，再根据 `SyncProgress` 展示加载进度。

---

## 检查点

任务可选择性做断点续传：

- **加载**：`ctx.load_checkpoint().await` → `Option<SyncCheckpoint>`（含 `task_id`、`cursor`）。  
- **保存**：任务返回 `SyncTaskResult::ok_with_cursor(cursor)`，或执行中调用 `ctx.save_checkpoint(Some(cursor)).await`。

检查点存于 `SyncCursorStore`，key 为 `sync:checkpoint:{task_id}`。

---

## 业务同步任务

本模块仅定义**核心引擎**（任务抽象、编排、协议同步）。会话/消息/已读等业务任务在 [crate::application::sync_task] 中实现，可按需注册。

---

## 入口与流程

- **入口**：`SyncManager` 由 `SdkEngine` 构造，通过 `engine.sync_manager()` 获取。  
- **注册**：`register_task_arc(Arc<dyn SyncTask>)` 或 `IMClientBuilder::add_sync_task` / `add_sync_task_arc`；构建时默认已注册 `ConversationsSyncTask`、`MessagesSyncTask`。  
- **执行**：仅在 `SdkEngine::bootstrap` 内调用 `sync_manager.run_sync(user_id)`，外部不直接调用 `run_sync`。  
- **单会话与已读**：`IMClient::sync_conversation(conv_id)` / `conversation.mark_read` 通过 `engine.session_sync_runner()` 调用 [SessionSyncRunner]。会话列表全量同步由同步引擎在连接后自动执行，不暴露给上层。

