# 事件系统架构设计（DDD + CQRS）

## 架构原则

按照 **DDD + CQRS** 原则，事件系统分为三层：

```
┌─────────────────────────────────────────────────────────┐
│  Interface Layer (接口层)                                │
│  - 适配器：SubscriberBuilder（链式 API）                 │
│  - 职责：提供便捷的用户接口                               │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│  Infrastructure Layer (基础设施层)                        │
│  - EventBus：事件总线实现（基于 tokio broadcast）        │
│  - SubscriptionManager：订阅管理器实现                    │
│  - Filter：事件过滤器实现                                 │
│  - SubscriptionEntry：订阅者条目数据结构                  │
│  - 职责：实现事件总线的具体功能                            │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│  Domain Layer (领域层)                                   │
│  - Subscribers：事件订阅器 trait 定义（领域接口）        │
│  - DomainEvent：领域事件定义                              │
│  - 职责：定义领域模型的事件订阅契约，不依赖任何基础设施    │
└─────────────────────────────────────────────────────────┘
```

## 模块分布

### Domain 层 (`domain/event/`)

**职责**: 定义领域接口，不依赖任何基础设施

- `subscribers.rs`: 事件订阅器 trait 定义
  - `ConnectionEventSubscriber`
  - `SessionEventSubscriber`
  - `MessageEventSubscriber`
  - `ConversationEventSubscriber`
  - `SyncEventSubscriber`

**特点**:
- ✅ 纯领域接口，无基础设施依赖
- ✅ 类型安全，编译期保证
- ✅ 可独立测试

### Infrastructure 层 (`infrastructure/event_bus/`)

**职责**: 实现事件总线的具体功能

- `event_bus.rs`: 事件总线实现（基于 tokio broadcast）
- `subscription_manager.rs`: 订阅管理器实现
- `filter.rs`: 事件过滤器实现
- `subscription_entry.rs`: 订阅者条目数据结构

**特点**:
- ✅ 依赖 tokio、tracing 等基础设施框架
- ✅ 实现领域接口定义的功能
- ✅ 可替换实现（如 Redis EventBus、Kafka EventBus）

### Interface 层 (`interface/event/`)

**职责**: 提供适配器，简化用户使用

- `subscriber_builder.rs`: 订阅器构建器（链式 API）

**特点**:
- ✅ 轻量级适配器，不包含业务逻辑
- ✅ 提供便捷的用户接口
- ✅ 隐藏基础设施细节

## 依赖关系

```
Interface Layer
    ↓ (使用)
Infrastructure Layer
    ↓ (实现)
Domain Layer (trait)
```

**关键原则**:
- Domain 层不依赖任何其他层
- Infrastructure 层实现 Domain 层定义的接口
- Interface 层使用 Infrastructure 层，提供适配器

## 使用示例

### 方式一：使用领域接口（推荐）

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

### 方式二：使用基础设施（高级用法）

```rust
use flare_im_core_sdk::infrastructure::event_bus::EventBus;

let event_bus = EventBus::new(1000);
event_bus.subscribe_message(Arc::new(MySubscriber)).await;
```

### 方式三：使用接口适配器（便捷）

```rust
use flare_im_core_sdk::interface::event::SubscriberBuilder;

sdk.events()
    .subscribe_events()
    .message(Arc::new(MySubscriber))
    .build()
    .await;
```

## 架构优势

1. **职责清晰**: 每层职责明确，符合单一职责原则
2. **依赖倒置**: Domain 层定义接口，Infrastructure 层实现
3. **可测试性**: Domain 层可独立测试，Infrastructure 层可 mock
4. **可扩展性**: 可以轻松替换 Infrastructure 实现（如 Redis EventBus）
5. **轻量级 Interface**: Interface 层只负责适配，不包含业务逻辑

## 迁移指南

### 从旧版本迁移

旧代码：
```rust
use flare_im_core_sdk::interface::event::subscribers::*;
```

新代码（推荐）：
```rust
use flare_im_core_sdk::domain::event::subscribers::*;
```

或者（向后兼容）：
```rust
use flare_im_core_sdk::interface::event::*;  // 重新导出
```

### 内部代码迁移

**Application 层**:
```rust
// 旧
use crate::interface::event::EventBus;

// 新
use crate::infrastructure::event_bus::EventBus;
```

**Infrastructure 层**:
```rust
// 旧
use crate::interface::event::subscribers::*;

// 新
use crate::domain::event::subscribers::*;
```

## 总结

通过 DDD + CQRS 重构，事件系统现在：
- ✅ **Domain 层**: 定义领域接口，无依赖
- ✅ **Infrastructure 层**: 实现具体功能
- ✅ **Interface 层**: 提供适配器，轻量级

这样的架构更符合 DDD 原则，职责清晰，易于维护和扩展。
