# 事件系统 DDD + CQRS 重构总结

## 重构目标

按照 **DDD + CQRS** 原则，将 `interface/event` 模块进行合理拆分，减轻 Interface 层负担。

## 重构前的问题

1. **Interface 层过重**: `interface/event` 包含了约 1500+ 行代码
2. **职责不清**: Interface 层包含了基础设施实现和领域接口
3. **违反 DDD 原则**: 领域接口和基础设施实现混在一起

## 重构后的架构

### Domain 层 (`domain/event/subscribers.rs`)

**职责**: 定义领域接口（trait），不依赖任何基础设施

- ✅ `ConnectionEventSubscriber` trait
- ✅ `SessionEventSubscriber` trait
- ✅ `MessageEventSubscriber` trait
- ✅ `ConversationEventSubscriber` trait
- ✅ `SyncEventSubscriber` trait

**特点**:
- 纯领域接口，无基础设施依赖
- 可独立测试
- 符合 DDD 原则

### Infrastructure 层 (`infrastructure/event_bus/`)

**职责**: 实现事件总线的具体功能

- ✅ `event_bus.rs`: 事件总线实现（基于 tokio broadcast）
- ✅ `subscription_manager.rs`: 订阅管理器实现
- ✅ `filter.rs`: 事件过滤器实现
- ✅ `subscription_entry.rs`: 订阅者条目数据结构

**特点**:
- 依赖 tokio、tracing 等基础设施框架
- 实现领域接口定义的功能
- 可替换实现（如 Redis EventBus、Kafka EventBus）

### Interface 层 (`interface/event/`)

**职责**: 提供适配器，简化用户使用

- ✅ `subscriber_builder.rs`: 订阅器构建器（链式 API）

**特点**:
- 轻量级适配器，约 100 行代码
- 提供便捷的用户接口
- 隐藏基础设施细节

## 文件迁移

### 移动到 Domain 层
- `interface/event/subscribers.rs` → `domain/event/subscribers.rs`

### 移动到 Infrastructure 层
- `interface/event/event_bus.rs` → `infrastructure/event_bus/event_bus.rs`
- `interface/event/subscription_manager.rs` → `infrastructure/event_bus/subscription_manager.rs`
- `interface/event/filter.rs` → `infrastructure/event_bus/filter.rs`
- `interface/event/subscription_entry.rs` → `infrastructure/event_bus/subscription_entry.rs`

### 保留在 Interface 层
- `interface/event/subscriber_builder.rs` (适配器)

## 代码行数对比

| 层级 | 重构前 | 重构后 | 变化 |
|------|--------|--------|------|
| Interface 层 | ~1500 行 | ~100 行 | ✅ 减少 93% |
| Infrastructure 层 | 0 行 | ~1500 行 | ✅ 新增 |
| Domain 层 | 0 行 | ~400 行 | ✅ 新增 |

## 依赖关系

```
Interface Layer (适配器)
    ↓ (使用)
Infrastructure Layer (实现)
    ↓ (实现)
Domain Layer (trait 定义)
```

**关键原则**:
- ✅ Domain 层不依赖任何其他层
- ✅ Infrastructure 层实现 Domain 层定义的接口
- ✅ Interface 层使用 Infrastructure 层，提供适配器

## 使用方式（向后兼容）

### 推荐方式（使用领域接口）

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

### 便捷方式（使用接口适配器）

```rust
use flare_im_core_sdk::interface::event::SubscriberBuilder;

sdk.events()
    .subscribe_events()
    .message(Arc::new(MySubscriber))
    .build()
    .await;
```

### 向后兼容（重新导出）

```rust
// 仍然可以通过 interface::event 访问（重新导出）
use flare_im_core_sdk::interface::event::*;
```

## 架构优势

1. **职责清晰**: 每层职责明确，符合单一职责原则
2. **依赖倒置**: Domain 层定义接口，Infrastructure 层实现
3. **可测试性**: Domain 层可独立测试，Infrastructure 层可 mock
4. **可扩展性**: 可以轻松替换 Infrastructure 实现
5. **轻量级 Interface**: Interface 层只负责适配，不包含业务逻辑

## 总结

通过 DDD + CQRS 重构：
- ✅ **Interface 层**: 从 1500+ 行减少到 100 行（减少 93%）
- ✅ **职责清晰**: 每层职责明确，符合 DDD 原则
- ✅ **向后兼容**: 通过重新导出保持 API 兼容性
- ✅ **易于维护**: 代码组织更合理，易于理解和修改
