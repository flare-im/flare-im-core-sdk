# 事件系统使用指南（DDD + CQRS 架构）

## 架构概述

按照 **DDD + CQRS** 原则，事件系统分为三层：

```
Interface Layer (接口适配层)
    ↓ 使用
Infrastructure Layer (基础设施实现层)
    ↓ 实现
Domain Layer (领域接口层)
```

## 模块分布

### Domain 层 (`domain/event/subscribers.rs`)

**职责**: 定义领域接口（trait），不依赖任何基础设施

**内容**:
- `ConnectionEventSubscriber` - 连接事件订阅器
- `SessionEventSubscriber` - 会话事件订阅器
- `MessageEventSubscriber` - 消息事件订阅器
- `ConversationEventSubscriber` - 会话事件订阅器
- `SyncEventSubscriber` - 同步事件订阅器

**使用方式**:
```rust
use flare_im_core_sdk::domain::event::subscribers::*;

struct MySubscriber;
#[async_trait::async_trait]
impl MessageEventSubscriber for MySubscriber {
    async fn on_message_delivered(&self, event: &MessageDelivered) -> anyhow::Result<()> {
        // 处理消息
        Ok(())
    }
}
```

### Infrastructure 层 (`infrastructure/event_bus/`)

**职责**: 实现事件总线的具体功能

**模块**:
- `event_bus.rs` - 事件总线实现（基于 tokio broadcast）
- `subscription_manager.rs` - 订阅管理器实现
- `filter.rs` - 事件过滤器实现
- `subscription_entry.rs` - 订阅者条目数据结构

**使用方式**:
```rust
use flare_im_core_sdk::infrastructure::event_bus::EventBus;

let event_bus = EventBus::new(1000);
event_bus.subscribe_message(Arc::new(MySubscriber)).await;
```

### Interface 层 (`interface/event/`)

**职责**: 提供适配器，简化用户使用

**内容**:
- `subscriber_builder.rs` - 订阅器构建器（链式 API）

**使用方式**:
```rust
use flare_im_core_sdk::interface::event::SubscriberBuilder;

sdk.events()
    .subscribe_events()
    .message(Arc::new(MySubscriber))
    .build()
    .await;
```

## 推荐使用方式

### 方式一：使用 Facade API（最推荐）

```rust
// 使用 SDK 提供的便捷 API
let event_facade = sdk.events();
let subscriber_id = event_facade.subscribe_message(Arc::new(MySubscriber)).await;
```

### 方式二：使用链式构建器

```rust
sdk.events()
    .subscribe_events()
    .message(Arc::new(MyMessageSubscriber))
    .connection(Arc::new(MyConnectionSubscriber))
    .build()
    .await;
```

### 方式三：直接使用领域接口

```rust
use flare_im_core_sdk::domain::event::subscribers::*;

// 实现订阅器
struct MySubscriber;
#[async_trait::async_trait]
impl MessageEventSubscriber for MySubscriber {
    // ...
}

// 注册
sdk.events().subscribe_message(Arc::new(MySubscriber)).await;
```

## 向后兼容

为了保持向后兼容，`interface::event` 模块重新导出了所有类型：

```rust
// 以下两种方式都可以使用
use flare_im_core_sdk::domain::event::subscribers::*;  // 推荐
use flare_im_core_sdk::interface::event::*;            // 向后兼容
```

## 架构优势

1. **职责清晰**: 每层职责明确，符合单一职责原则
2. **依赖倒置**: Domain 层定义接口，Infrastructure 层实现
3. **可测试性**: Domain 层可独立测试，Infrastructure 层可 mock
4. **可扩展性**: 可以轻松替换 Infrastructure 实现
5. **轻量级 Interface**: Interface 层只负责适配，不包含业务逻辑

## 相关文档

- `ARCHITECTURE.md`: 详细的架构设计说明
- `DDD_REFACTORING.md`: DDD + CQRS 重构报告
- `REFACTORING_SUMMARY.md`: 重构总结
