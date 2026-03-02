# Flare IM Core SDK - 存储实现

本目录包含 Flare IM Core SDK 的存储实现。

## 架构设计

SDK 核心（`flare-im-core-sdk/src`）只包含业务逻辑和 trait 定义，不包含具体的存储实现。存储实现由独立的 crate 提供，用户可以根据平台选择合适的实现。

## 目录结构

```
storage/
├── sqlite/          # SQLite 存储实现（桌面/移动平台）
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── event_store.rs
│       ├── message_repository.rs
│       ├── conversation_repository.rs
│       └── snapshot_store.rs
└── indexeddb/       # IndexedDB 存储实现（Web 平台，待实现）
    └── README.md
```

## SQLite 存储实现

### 特性

- ✅ **EventStore**: 事件存储（事件溯源）
- ✅ **MessageRepository**: 消息仓储（支持分页、搜索、时间范围查询）
- ✅ **ConversationRepository**: 会话仓储（支持分页、参与者查询）
- ✅ **SnapshotStore**: 快照存储（可选，用于性能优化）

### 使用方式

```rust
use flare_im_core_sdk_storage_sqlite::create_storage;
use flare_im_core_sdk::interface::facade::ImCoreSdk;
use std::sync::Arc;

// 创建存储实例
let database_url = "sqlite:./flare_im.db";
let (event_store, message_repository, conversation_repository) = 
    create_storage(database_url).await?;

// 创建 SDK 实例
let sdk = Arc::new(ImCoreSdk::new(
    config,
    event_store as Arc<dyn EventStore>,
    message_repository as Arc<dyn MessageRepository>,
    conversation_repository as Arc<dyn ConversationRepository>,
).await?);
```

### 数据库表结构

#### events 表
- `event_id`: 事件ID（主键）
- `event_type`: 事件类型
- `aggregate_id`: 聚合根ID
- `version`: 版本号
- `timestamp`: 时间戳
- `data`: 事件数据（JSON）

#### messages 表
- `client_msg_id`: 客户端消息ID（主键）
- `server_id`: 服务端消息ID
- `conversation_id`: 会话ID
- `sender_id`: 发送者ID
- `content`: 消息内容（BLOB）
- `timestamp`: 时间戳
- ... 其他字段

#### conversations 表
- `conversation_id`: 会话ID（主键）
- `conversation_type`: 会话类型
- `display_name`: 显示名称
- `unread_count`: 未读数
- `max_seq`: 最大序列号
- ... 其他字段

#### snapshots 表
- `aggregate_id`: 聚合根ID
- `aggregate_type`: 聚合根类型
- `version`: 版本号
- `data`: 快照数据（JSON）

### 索引

所有表都创建了必要的索引以优化查询性能：
- `events`: `(aggregate_id)`, `(aggregate_id, version)`
- `messages`: `(server_id)`, `(conversation_id)`, `(timestamp DESC)`, `(conversation_id, timestamp DESC)`
- `conversations`: `(updated_at DESC)`

## IndexedDB 存储实现

待实现。适用于 Web 平台（wasm32），使用浏览器的 IndexedDB API。

## 设计原则

1. **依赖倒置**: SDK 核心只依赖 trait，不依赖具体实现
2. **平台无关**: 核心代码不关心存储的具体实现方式
3. **灵活扩展**: 用户可以轻松实现自己的存储（如 PostgreSQL、MongoDB 等）
4. **性能优化**: 每个实现都可以针对特定平台进行优化

## 实现自己的存储

要实现自己的存储，需要实现以下 trait：

- `EventStore` - 事件存储
- `MessageRepository` - 消息仓储
- `ConversationRepository` - 会话仓储
- `SnapshotStore` - 快照存储（可选）

参考 `sqlite/` 目录中的实现作为示例。
