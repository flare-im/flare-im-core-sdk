# Flare IM Core SDK - Tauri Bindings

Tauri 桌面应用专用的 SDK 绑定层，提供完整的 IM 功能：消息、会话、事件订阅等。

## 设计原则

1. **异步优先**：所有操作使用异步，通过 Tauri 事件系统通知前端
2. **架构清晰**：按功能模块拆分文件，职责单一
3. **类型安全**：直接使用 SDK 领域模型，无需额外转换
4. **事件驱动**：通过事件订阅器自动转发 SDK 事件到前端

## 目录结构

```
bindings/tauri/
├── Cargo.toml          # 依赖配置
├── README.md           # 本文档
└── src/
    ├── lib.rs          # 主入口，导出模块和注册函数
    ├── state.rs        # SDK 状态管理
    ├── utils.rs        # 工具函数（路径查找、Token 生成等）
    ├── commands/       # Tauri 命令实现
    │   ├── mod.rs
    │   ├── lifecycle.rs    # 生命周期（init, connect, login, logout）
    │   ├── message.rs     # 消息操作（发送、编辑、删除、撤回、反应等）
    │   ├── conversation.rs # 会话操作（获取列表、标记已读、输入状态等）
    │   └── sync.rs         # 同步操作（Bootstrap Sync、增量同步等）
    └── events/          # 事件转发
        ├── mod.rs
        ├── message.rs      # 消息事件转发
        ├── connection.rs   # 连接事件转发
        ├── session.rs      # 会话事件转发
        ├── conversation.rs # 会话事件转发
        └── sync.rs         # 同步事件转发
```

## 快速开始

### 1. 添加依赖

在 Tauri 应用的 `Cargo.toml` 中添加：

```toml
[dependencies]
flare-im-core-sdk-tauri = { path = "../../bindings/tauri" }
```

### 2. 注册命令和状态

在 Tauri 应用的 `main.rs` 或 `lib.rs` 中：

```rust
use flare_im_core_sdk_tauri::{commands::*, state::SdkState, register_event_subscribers};
use flare_im_core_sdk::interface::facade::ImCoreSdk;
use flare_im_core_sdk::config::SdkConfigBuilder;

fn main() {
    tauri::Builder::default()
        .manage(SdkState::default())
        .invoke_handler(tauri::generate_handler![
            // 生命周期
            lifecycle::sdk_init,
            lifecycle::sdk_logout,
            
            // 消息操作
            message::sdk_send_text_message,
            message::sdk_get_messages,
            // ... 其他命令
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**注意**：由于 Rust 宏的限制，无法直接返回 handler 函数。请在 `invoke_handler` 中直接使用 `tauri::generate_handler!` 宏，导入所有命令模块。

### 3. 初始化 SDK

在前端调用：

```typescript
// 初始化 SDK（可选：environment、sdk_config）。不传 db_path，按 environment 自动解析数据目录
await invoke('sdk_init', {
  environment: import.meta.env.DEV ? 'development' : 'production',
  sdk_config: undefined,  // 可选：含 serverUrl 及超时、重连等，见下表
});

// 登录（内部会创建存储、连接并启动事件转发）
await invoke('sdk_login', { userId: 'user123', token: '…' });
```

- **数据目录**：`development` 使用项目下 **temp-data**（flare-im-core-sdk/temp-data）；`production` 使用 Tauri 应用目录（app_data_dir 等）。
- **ws_url**：放在 `sdk_config.wsUrl` 中；未传时使用环境变量 `FLARE_IM_SERVER_URL` 或默认 `ws://localhost:60051`。

**个性化配置 `sdk_config`**（可选）：字段均为可选，camelCase，与 Rust `SdkConfig` 对应：

| 字段 | 类型 | 说明 |
|------|------|------|
| `wsUrl` | string | WebSocket 地址 |
| `quicUrl` | string | QUIC 端点 |
| `httpUrl` | string | HTTP 端点 |
| `connectTimeoutSecs` | number | 连接超时（秒） |
| `reconnectIntervalSecs` | number | 重连间隔（秒） |
| `maxReconnectAttempts` | number | 最大重连次数 |
| `syncBatchSize` | number | 同步批次大小 |
| `ackTimeoutSecs` | number | 回执超时（秒） |
| `ackMaxRetries` | number | 回执最大重试 |
| `enableMetrics` | boolean | 是否开启指标 |

### 4. 监听事件

在前端监听 SDK 事件：

```typescript
import { listen } from '@tauri-apps/api/event';

// 监听消息事件
listen('im://message', (event) => {
  const message = event.payload;
  console.log('收到消息:', message);
});

// 监听连接状态
listen('im://connection_state', (event) => {
  const state = event.payload;
  console.log('连接状态:', state);
});
```

## API 文档

### 生命周期命令

- `sdk_init(environment?, sdk_config?)` - 初始化 SDK；数据目录按 environment（开发 temp-data，生产 Tauri 目录）；ws_url 在 sdk_config.wsUrl
- `sdk_login(args)` - 登录；args 为 `{ userId, token }`（camelCase），内部创建存储、连接并启动事件转发；对外仅暴露此命令
- `sdk_logout()` - 登出

### 消息命令

**约定**：所有对上层暴露的 `message_id` 均为 **client_msg_id**（客户端生成的消息 ID）；server_msg_id 仅用于内部与服务端交互，前端无需关心。

- `sdk_send_text_message(session_id: String, text: String)` - 发送文本消息
- `sdk_edit_message(session_id: String, message_id: String, text: String)` - 编辑消息（message_id 为 client_msg_id）
- `sdk_delete_message(message_id: String, delete_type?: i32, reason?: String)` - 删除消息
- `sdk_recall_message(message_id: String)` - 撤回消息
- `sdk_add_reaction(message_id: String, emoji: String)` - 添加反应
- `sdk_remove_reaction(message_id: String, emoji: String)` - 移除反应
- `sdk_forward_message(message_ids: Vec<String>, target_session_id: String, merge_forward: bool, reason?: String)` - 转发消息
- `sdk_quote_message(session_id: String, quoted_message_id: String, text: String, preview_text?: String)` - 引用消息
- `sdk_reply_message(session_id: String, reply_to_message_id: String, text: String)` - 回复消息
- `sdk_pin_message(message_id: String, reason?: String, expire_at?: i64)` - 置顶消息
- `sdk_unpin_message(message_id: String)` - 取消置顶消息
- `sdk_favorite_message(message_id: String, tags?: Vec<String>, note?: String)` - 收藏消息
- `sdk_unfavorite_message(message_id: String)` - 取消收藏消息
- `sdk_mark_message(message_id: String, mark_type: i32, color?: String)` - 标记消息
- `sdk_get_messages(session_id: String, limit: usize, cursor?: String)` - 获取消息列表
- `sdk_send_with_media_progress(message: IMMessage)` - 发送消息并推送媒体上传进度事件（本地媒体路径场景）

### 会话命令

- `sdk_get_sessions(unread_only?: bool, limit?: usize, offset?: usize)` - 获取会话列表
- `sdk_create_session(session_type: String, business_type: String, display_name?: String, peer_id?: String, current_user_id?: String)` - 创建会话
- `sdk_mark_session_read(session_id: String, last_seq?: i64)` - 标记会话已读
- `sdk_mark_all_read()` - 标记所有会话已读
- `sdk_send_typing(session_id: String, action: String)` - 发送输入状态

### 同步命令

- `sdk_sync_session_incremental(session_id: String)` - 单会话增量同步消息（会话列表由同步引擎在连接后自动同步，不暴露给前端）
- `sdk_bootstrap_sync()` - 执行 Bootstrap Sync

## 事件列表

### 消息事件

- `im://message` - 消息创建/更新（包含完整消息对象）
- `im://upload_progress` - 媒体上传进度（用于图片/视频/音频/文件发送进度）
- `im://message_delivered` - 消息已送达
- `im://message_read` - 消息已读
- `im://message_recalled` - 消息已撤回
- `im://message_edited` - 消息已编辑
- `im://message_deleted` - 消息已删除
- `im://message_pinned` - 消息已置顶
- `im://message_unpinned` - 消息已取消置顶
- `im://message_favorited` - 消息已收藏
- `im://message_unfavorited` - 消息已取消收藏
- `im://message_marked` - 消息已标记
- `im://message_unmarked` - 消息已取消标记
- `im://message_forwarded` - 消息已转发
- `im://message_replied` - 消息已回复

### 连接事件

- `im://connection_state` - 连接状态变化（Connected/Disconnected/Reconnecting/Reconnected/ConnectFailed）

### 会话事件

- `im://session_logged_in` - 登录成功
- `im://session_logged_out` - 登出
- `im://session_expired` - 会话过期
- `im://token_refreshed` - Token 刷新

### 会话事件

- `im://conversation_created` - 会话创建
- `im://unread` - 未读数更新
- `im://conversation_last_message_updated` - 最后一条消息更新
- `im://conversation_marked_as_read` - 会话标记为已读
- `im://conversation_draft_updated` - 草稿更新
- `im://conversation_hidden` - 会话隐藏
- `im://conversation_all_hidden` - 所有会话隐藏
- `im://conversation_deleted` - 会话删除
- `im://conversation_messages_cleared` - 会话消息清空
- `im://conversation_updated` - 会话更新
- `im://conversation_muted` - 会话静音
- `im://conversation_unmuted` - 会话取消静音
- `im://conversation_pinned` - 会话置顶
- `im://conversation_unpinned` - 会话取消置顶
- `im://conversation_archived` - 会话归档
- `im://conversation_unarchived` - 会话取消归档
- `im://conversation_input_state_updated` - 输入状态更新
- `im://conversation_input_state_cleared` - 输入状态清除

### 同步事件

- `im://sync_bootstrap_started` - Bootstrap 同步开始
- `im://sync_bootstrap_completed` - Bootstrap 同步完成
- `im://sync_bootstrap_failed` - Bootstrap 同步失败
- `im://sync_async_started` - 异步同步开始
- `im://sync_async_completed` - 异步同步完成
- `im://sync_async_failed` - 异步同步失败
- `im://sync_progress_updated` - 同步进度更新

## 配置

### 环境变量

- `FLARE_IM_SERVER_URL` - WebSocket 地址（未在 sdk_config.wsUrl 中指定时使用，默认：`ws://localhost:60051`）
- `FLARE_IM_USE_TLS` - 是否使用 TLS（`1` 表示启用）
- 数据目录由 `sdk_init` 的 `environment` 决定：开发用 **temp-data**，生产用 Tauri 应用目录（不通过环境变量指定）
- `FLARE_IM_TOKEN` - 认证 Token（可选，未设置时自动生成）
- `TOKEN_SECRET` - JWT Secret（默认：`insecure-secret`）
- `TOKEN_ISSUER` - JWT Issuer（默认：`flare-im-core`）
- `TOKEN_TTL_SECONDS` - Token 有效期（默认：3600 秒）
- `TENANT_ID` - 租户 ID（可选）

## 示例

完整示例请参考 `examples/tauri`。

## 媒体上传进度协议

`im://upload_progress` 的统一字段、阶段状态机与跨端兼容规范见：

- [媒体上传进度事件协议](../../docs/upload_progress_event_protocol.md)

## 注意事项

1. **消息内容解码**：由 **SDK 核心层**（`flare_im_core_sdk::model::message_elem`）统一提供 `Elem` 与 `decoded_content_to_elem`，绑定层在转换 Message 时调用并填充 `contentDecoded`。前端可根据 `contentDecoded.contentType` 取得对应结构，无需解析原始字节。兼容保留 `content` 与 `extra.content_text`。

2. **事件重试机制**：消息事件（创建、发送、编辑等）会自动重试查询消息（最多 3 次），确保前端能收到完整的消息对象。

3. **异步处理**：所有事件处理都在独立的 Tokio 任务中执行，不会阻塞 SDK 的事件处理流程。

4. **状态管理**：SDK 状态通过 Tauri 的 `State` 管理，支持多线程访问。

## 许可证

MIT
