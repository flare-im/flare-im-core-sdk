# Application 层架构设计（DDD + CQRS）

## 架构概述

Application 层是业务组织层，负责编排领域服务，处理应用层逻辑，不包含业务规则。

## 目录结构

```
application/
├── mod.rs                    # 模块导出
├── commands/                 # 命令定义（CQRS 写侧）
│   ├── mod.rs
│   ├── connection.rs         # 连接相关命令
│   ├── message.rs            # 消息相关命令
│   ├── session.rs            # 会话相关命令
│   └── sync.rs               # 同步相关命令
├── queries/                  # 查询定义（CQRS 读侧）
│   ├── mod.rs
│   ├── message.rs
│   ├── session.rs
│   └── sync.rs
├── handlers/                 # 命令和查询处理器
│   ├── mod.rs
│   ├── connection_command_handler.rs
│   ├── message_command_handler.rs
│   ├── message_query_handler.rs
│   ├── session_command_handler.rs
│   ├── session_query_handler.rs
│   ├── sync_command_handler.rs
│   └── sync_query_handler.rs
├── services/                 # 应用服务（业务编排）
│   ├── mod.rs
│   ├── connection_service.rs
│   ├── message_service.rs    # 消息应用服务
│   ├── session_service.rs
│   └── sync_service.rs
├── receivers/                # 服务端消息/命令接收处理
│   ├── mod.rs
│   ├── message_receiver.rs   # 处理服务端推送的消息
│   └── command_receiver.rs   # 处理服务端推送的命令
└── crypto.rs                 # 加密服务
```

## 设计原则

### 1. CQRS 严格分离

- **命令（Command）**：所有写操作，定义在 `commands/` 目录
- **查询（Query）**：所有读操作，定义在 `queries/` 目录
- **处理器（Handler）**：分别处理命令和查询，互不干扰

### 2. 薄应用层

- **只负责编排**：不包含业务逻辑，业务逻辑在 Domain 层
- **协调领域服务**：调用 DomainService 和 Repository
- **发布领域事件**：通过 EventBus 发布事件

### 3. 无状态设计

- 所有 Handler 和 Service 都是无状态的
- 可以并发使用，无需加锁
- 状态存储在 Domain 层或 Infrastructure 层

### 4. 事件驱动

- 通过 EventBus 解耦各模块
- CommandHandler 处理完成后发布领域事件
- API 层和 Infrastructure 层订阅事件

## 数据流向

### 写操作（Command）流程

```
API 层
  ↓
Command (commands/)
  ↓
CommandHandler (handlers/)
  ↓
DomainService (domain/service/)
  ↓
Repository (domain/repository/)
  ↓
RepositoryImpl (infrastructure/persistence/)
  ↓
StorageBackend (infrastructure/storage/)
  ↓
EventBus (发布领域事件)
```

### 读操作（Query）流程

```
API 层
  ↓
Query (queries/)
  ↓
QueryHandler (handlers/)
  ↓
Repository (domain/repository/)
  ↓
RepositoryImpl (infrastructure/persistence/)
  ↓
StorageBackend (infrastructure/storage/)
  ↓
返回结果给 API 层
```

### 服务端推送流程

```
Infrastructure (收到 Frame)
  ↓
Receiver (receivers/)
  ↓
Service (services/) 或 CommandHandler (handlers/)
  ↓
DomainService / Repository
  ↓
EventBus (发布领域事件)
  ↓
API 层（通过事件订阅）
```

## 模块职责

### Commands（命令定义）

- **职责**：定义所有写操作的数据结构
- **原则**：只包含数据，不包含逻辑
- **示例**：`SendMessageCommand`, `CreateSessionCommand`

### Queries（查询定义）

- **职责**：定义所有读操作的数据结构
- **原则**：只包含查询参数，不包含逻辑
- **示例**：`GetMessagesQuery`, `GetSessionsQuery`

### Handlers（处理器）

- **职责**：处理命令和查询，协调领域服务
- **原则**：
  - CommandHandler 处理写操作，调用 DomainService
  - QueryHandler 处理读操作，调用 Repository
  - 不包含业务逻辑，只负责编排

### Services（应用服务）

- **职责**：编排多个 Handler，提供给 API 层使用
- **原则**：
  - 可以组合多个 Handler
  - 处理应用层逻辑（如事务、事件发布）
  - 不包含业务规则

### Receivers（接收器）

- **职责**：处理服务端推送的消息和命令
- **原则**：
  - MessageReceiver 处理服务端推送的消息
  - CommandReceiver 处理服务端推送的命令
  - 由 Infrastructure 层调用

## 使用示例

### API 层调用

```rust
// API 层（api/message.rs）
impl MessageApi for FlareIMClient {
    async fn send_message(&self, message: Message, ...) -> Result<String> {
        // 1. 构建命令
        let cmd = SendMessageCommand { ... };
        
        // 2. 调用 CommandHandler
        self.message_command_handler.handle_send_message(cmd).await
    }
}
```

### 服务端推送处理

```rust
// Infrastructure 层（infrastructure/handler/message.rs）
impl MessageFrameHandler {
    async fn handle_message_command(&self, msg_cmd: &MessageCommand) -> Result<()> {
        // 1. 解析消息
        let message = decode_message(msg_cmd)?;
        
        // 2. 调用 MessageReceiver
        self.message_receiver.receive(message).await
    }
}
```

## 参考设计

- **微信 SDK**：Command/Query 分离，Handler 处理
- **Telegram SDK**：Service 层编排，Receiver 处理推送
- **飞书 SDK**：CQRS 严格分离，事件驱动
- **Discord SDK**：Handler 模式，Service 编排
