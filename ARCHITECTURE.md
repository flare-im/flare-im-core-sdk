# Flare IM Core SDK 架构设计文档

## 架构概述

Flare IM Core SDK 采用**领域驱动设计（DDD）+ 命令查询责任分离（CQRS）**架构，分为四层：

```
┌─────────────────────────────────────────────────────────┐
│                    API 层                                │
│  (api/) - 对外接口，实现 traits.rs 定义的 API           │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│              Application 层                             │
│  (application/) - 业务编排，CQRS 分离                   │
│  ├── commands/     - 命令定义（写操作）                 │
│  ├── queries/      - 查询定义（读操作）                 │
│  ├── handlers/     - 命令和查询处理器                   │
│  ├── services/     - 应用服务（业务编排）                │
│  └── receivers/    - 服务端消息/命令接收                │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│                  Domain 层                               │
│  (domain/) - 业务核心，领域模型和业务规则               │
│  ├── message/     - 消息领域（聚合根、事件、服务）      │
│  ├── session/     - 会话领域（聚合根、事件、服务）      │
│  └── sync/        - 同步领域（聚合根、事件、服务）      │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│            Infrastructure 层                            │
│  (infrastructure/) - 技术实现                           │
│  ├── connection/  - 连接管理                           │
│  ├── storage/     - 存储实现                           │
│  ├── event/       - 事件总线                           │
│  ├── handler/     - 消息帧处理                         │
│  └── persistence/ - 仓储实现                           │
└─────────────────────────────────────────────────────────┘
```

## 全链路流程

### 1. 发送消息流程

```
API 层 (api/message.rs)
  ↓ send_message()
Application 层 (application/services/message_service.rs)
  ↓ send_message()
Application 层 (application/handlers/message_command_handler.rs)
  ↓ handle_send_message()
Domain 层 (domain/message/service.rs)
  ↓ create_message()
Domain 层 (domain/message/model.rs)
  ↓ send() -> MessageSentEvent
Infrastructure 层 (infrastructure/persistence/storage/)
  ↓ save() -> Repository
Infrastructure 层 (infrastructure/connection/)
  ↓ send_frame() -> ConnectionManager
EventBus
  ↓ publish(MessageCreated)
```

### 2. 接收消息流程

```
Infrastructure 层 (infrastructure/handler/message.rs)
  ↓ handle_frame() -> MessageFrameHandler
Application 层 (application/receivers/message_receiver.rs)
  ↓ receive()
Application 层 (application/services/message_service.rs)
  ↓ on_message_received()
Domain 层 (domain/message/model.rs)
  ↓ receive() -> MessageReceivedEvent
Infrastructure 层 (infrastructure/persistence/storage/)
  ↓ save() -> Repository
EventBus
  ↓ publish(MessageReceived)
API 层 (通过事件订阅)
```

### 3. 查询消息流程

```
API 层 (api/message.rs)
  ↓ get_messages()
Application 层 (application/services/message_service.rs)
  ↓ get_messages()
Application 层 (application/handlers/message_query_handler.rs)
  ↓ handle_get_messages()
Infrastructure 层 (infrastructure/persistence/storage/)
  ↓ find_by_session() -> Repository
  ↓ 返回结果
API 层
```

## 设计原则

### 1. DDD 原则

- **聚合根**：Message、Session、Sync 是聚合根，封装业务逻辑
- **领域事件**：所有业务操作返回领域事件
- **领域服务**：封装复杂业务逻辑，不依赖基础设施
- **仓储接口**：定义在领域层，实现在基础设施层

### 2. CQRS 原则

- **命令（Command）**：所有写操作，定义在 `application/commands/`
- **查询（Query）**：所有读操作，定义在 `application/queries/`
- **严格分离**：CommandHandler 和 QueryHandler 完全分离

### 3. 依赖方向

```
API → Application → Domain ← Infrastructure
```

- API 层依赖 Application 层
- Application 层依赖 Domain 层
- Infrastructure 层实现 Domain 层定义的接口
- Domain 层不依赖任何其他层

### 4. 事件驱动

- **领域事件**：Domain 层生成，表示业务事件
- **基础设施事件**：Infrastructure 层定义，用于通知 API 层
- **事件总线**：解耦各模块，异步通信

## 代码组织

### API 层 (`src/api/`)

- `traits.rs`：定义所有 API trait
- `client.rs`：FlareIMClient 主入口
- `connection.rs`：连接管理 API 实现
- `message.rs`：消息管理 API 实现
- `session.rs`：会话管理 API 实现
- `sync.rs`：同步 API 实现
- `callback/`：回调桥接（用于 FFI）

### Application 层 (`src/application/`)

- `commands/`：命令定义（写操作）
- `queries/`：查询定义（读操作）
- `handlers/`：命令和查询处理器
- `services/`：应用服务（业务编排）
- `receivers/`：服务端消息/命令接收
- `message/`：消息辅助功能（Builder、MediaUpload、DomainService）

### Domain 层 (`src/domain/`)

- `message/`：消息领域（Model、Event、Service、Repository）
- `session/`：会话领域（Model、Event、Service、Repository）
- `sync/`：同步领域（Model、Event、Service、Repository）

### Infrastructure 层 (`src/infrastructure/`)

- `connection/`：连接管理实现
- `storage/`：存储实现（SQLite、IndexedDB）
- `event/`：事件总线实现
- `handler/`：消息帧处理
- `persistence/`：仓储实现

## 关键设计决策

### 1. 为什么使用 CQRS？

- **性能优化**：读写分离，可以独立优化
- **扩展性**：读模型和写模型可以独立扩展
- **清晰性**：命令和查询职责明确，代码更清晰

### 2. 为什么使用领域事件？

- **解耦**：各模块通过事件解耦
- **可追溯**：所有业务操作都有事件记录
- **扩展性**：新功能可以通过订阅事件实现

### 3. 为什么 Application 层这么薄？

- **职责单一**：只负责编排，不包含业务逻辑
- **可测试**：业务逻辑在 Domain 层，易于测试
- **可替换**：可以轻松替换 Application 层实现

## 最佳实践

1. **API 层**：只调用 Application 层，不直接调用 Domain 层
2. **Application 层**：只编排，不包含业务规则
3. **Domain 层**：纯业务逻辑，不依赖基础设施
4. **Infrastructure 层**：实现 Domain 层定义的接口

## 参考设计

- **微信 SDK**：CQRS + 事件驱动
- **Telegram SDK**：领域模型 + 仓储模式
- **飞书 SDK**：DDD + CQRS 严格分离
- **Discord SDK**：事件驱动 + 薄应用层
