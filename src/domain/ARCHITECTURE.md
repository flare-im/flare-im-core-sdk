# Domain 层架构设计（领域驱动设计）

## 架构概述

Domain 层是业务核心层，包含领域模型、领域事件、领域服务和仓储接口。

## 目录结构

```
domain/
├── mod.rs                    # 模块导出
├── message/                  # 消息领域
│   ├── mod.rs
│   ├── model.rs              # Message 聚合根
│   ├── event.rs              # 消息领域事件
│   ├── service.rs            # 消息领域服务接口
│   └── repository.rs         # 消息仓储接口
├── session/                  # 会话领域
│   ├── mod.rs
│   ├── model.rs              # Session 聚合根
│   ├── event.rs              # 会话领域事件
│   ├── service.rs            # 会话领域服务接口
│   └── repository.rs         # 会话仓储接口
├── sync/                     # 同步领域
│   ├── mod.rs
│   ├── model.rs              # Sync 聚合根
│   ├── event.rs              # 同步领域事件
│   ├── service.rs            # 同步领域服务接口
│   └── repository.rs         # 同步仓储接口
└── message_builder.rs        # 消息构建器
```

## 设计原则

### 1. 聚合根（Aggregate Root）

- **Message**：消息聚合根，封装消息的领域逻辑和行为
- **Session**：会话聚合根，封装会话的领域逻辑和行为
- **Sync**：同步聚合根，封装同步的领域逻辑和行为

### 2. 值对象（Value Object）

- **MessageId**：消息ID值对象
- **SessionId**：会话ID值对象
- **UserId**：用户ID值对象

### 3. 领域事件（Domain Event）

领域事件表示业务中发生的重要事情，具有以下特点：

- **不可变**：事件一旦创建就不能修改
- **时间戳**：所有事件都包含时间戳
- **聚合根ID**：包含相关的聚合根ID
- **业务语义**：事件名称清晰表达业务含义

#### 消息领域事件

- `MessageSentEvent`：消息已发送
- `MessageReceivedEvent`：消息已接收
- `MessageRecalledEvent`：消息已撤回
- `MessageDeletedEvent`：消息已删除
- `MessageReadEvent`：消息已读
- `MessageEditedEvent`：消息已编辑
- `MessageForwardedEvent`：消息已转发
- `MessageReactionAddedEvent`：消息反应已添加
- `MessageReactionRemovedEvent`：消息反应已移除
- `MessagePinnedEvent`：消息已置顶
- `MessageUnpinnedEvent`：消息已取消置顶
- `MessageFavoritedEvent`：消息已收藏
- `MessageUnfavoritedEvent`：消息已取消收藏

#### 会话领域事件

- `SessionCreatedEvent`：会话已创建
- `SessionUpdatedEvent`：会话已更新
- `SessionDeletedEvent`：会话已删除
- `SessionHiddenEvent`：会话已隐藏
- `SessionShownEvent`：会话已显示
- `SessionMarkedReadEvent`：会话已标记为已读
- `SessionDraftSetEvent`：会话草稿已设置
- `SessionTypingSentEvent`：会话输入状态已发送

#### 同步领域事件

- `SyncStartedEvent`：同步已开始
- `SyncCompletedEvent`：同步已完成
- `SyncFailedEvent`：同步失败

### 4. 领域行为（Domain Behavior）

聚合根通过领域行为封装业务逻辑：

#### Message 领域行为

- `send()`：发送消息，返回 `MessageSentEvent`
- `receive()`：接收消息，返回 `MessageReceivedEvent`
- `recall()`：撤回消息，返回 `MessageRecalledEvent`
- `delete()`：删除消息，返回 `MessageDeletedEvent`
- `edit()`：编辑消息，返回 `MessageEditedEvent`
- `forward()`：转发消息，返回 `MessageForwardedEvent`
- `add_reaction()`：添加反应，返回 `MessageReactionAddedEvent`
- `remove_reaction()`：移除反应，返回 `MessageReactionRemovedEvent`
- `pin()`：置顶消息，返回 `MessagePinnedEvent`
- `unpin()`：取消置顶，返回 `MessageUnpinnedEvent`
- `favorite()`：收藏消息，返回 `MessageFavoritedEvent`
- `unfavorite()`：取消收藏，返回 `MessageUnfavoritedEvent`

#### Session 领域行为

- `create()`：创建会话，返回 `SessionCreatedEvent`
- `update()`：更新会话，返回 `SessionUpdatedEvent`
- `delete()`：删除会话，返回 `SessionDeletedEvent`
- `hide()`：隐藏会话，返回 `SessionHiddenEvent`
- `show()`：显示会话，返回 `SessionShownEvent`
- `mark_read()`：标记已读，返回 `SessionMarkedReadEvent`
- `set_draft()`：设置草稿，返回 `SessionDraftSetEvent`
- `send_typing()`：发送输入状态，返回 `SessionTypingSentEvent`

#### Sync 领域行为

- `start()`：开始同步，返回 `SyncStartedEvent`
- `complete()`：完成同步，返回 `SyncCompletedEvent`
- `fail()`：失败同步，返回 `SyncFailedEvent`
- `update_status()`：更新状态

### 5. 领域服务（Domain Service）

领域服务封装复杂的业务逻辑，不依赖基础设施：

- **MessageDomainService**：消息领域服务
  - `create_message()`：创建消息
  - `validate_message()`：验证消息
  - `generate_message_id()`：生成消息ID
  - `create_forward_message()`：创建转发消息

- **SessionDomainService**：会话领域服务
  - `create_session()`：创建会话
  - `validate_session()`：验证会话
  - `generate_session_id()`：生成会话ID

- **SyncDomainService**：同步领域服务
  - `create_sync()`：创建同步
  - `validate_sync()`：验证同步
  - `create_cursor()`：创建同步游标

### 6. 仓储接口（Repository Interface）

仓储接口定义在领域层，实现在基础设施层：

- **MessageRepository**：消息仓储接口
- **SessionRepository**：会话仓储接口
- **SyncRepository**：同步仓储接口

## 业务规则示例

### 消息撤回规则

```rust
pub fn recall(self, current_user_id: &UserId, reason: Option<String>) -> Result<MessageRecalledEvent> {
    // 业务规则：只能撤回自己的消息
    if &self.sender_id != current_user_id {
        return Err(MessageError::NotAuthorized.into());
    }

    // 业务规则：只能撤回一定时间内的消息（2 分钟）
    const MAX_RECALL_DURATION_SECS: i64 = 120;
    // ... 时间检查逻辑
}
```

### 消息编辑规则

```rust
pub fn edit(self, editor_id: &UserId, new_content: String) -> Result<MessageEditedEvent> {
    // 业务规则：只能编辑自己的消息
    if &self.sender_id != editor_id {
        return Err(MessageError::NotAuthorized.into());
    }

    // 业务规则：只能编辑文本消息
    if self.message_type != MessageType::Text {
        return Err(MessageError::ValidationFailed("Only text messages can be edited".to_string()).into());
    }

    // 业务规则：只能编辑一定时间内的消息（5 分钟）
    const MAX_EDIT_DURATION_SECS: i64 = 300;
    // ... 时间检查逻辑
}
```

### 会话标记已读规则

```rust
pub fn mark_read(self, reader_id: UserId, message_seq: Option<i64>) -> Result<SessionMarkedReadEvent> {
    // 业务规则：message_seq 不能大于 max_seq
    if let Some(seq) = message_seq {
        if seq > self.proto_summary.max_seq {
            return Err(SessionError::ValidationFailed(
                format!("Message seq {} exceeds max seq {}", seq, self.proto_summary.max_seq)
            ).into());
        }
    }
    // ...
}
```

## 使用示例

### 创建消息并发送

```rust
let message = Message::new(
    message_id,
    session_id,
    sender_id,
    content,
    message_type,
);

// 验证消息
message.validate()?;

// 发送消息（返回领域事件）
let event = message.send()?;

// 发布事件到事件总线
event_bus.publish(event);
```

### 撤回消息

```rust
let message = repository.find_by_id(&message_id).await?;

// 撤回消息（返回领域事件）
let event = message.recall(current_user_id, None)?;

// 发布事件
event_bus.publish(event);
```

## 参考设计

- **微信 SDK**：领域事件驱动，业务规则封装在聚合根
- **Telegram SDK**：领域行为返回事件，事件总线分发
- **飞书 SDK**：严格的领域模型，业务规则验证
- **Discord SDK**：事件驱动架构，领域服务编排
