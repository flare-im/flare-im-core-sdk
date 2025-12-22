# 扩展模块开发指南

## 概述

Flare IM Core SDK 采用插件化架构，通过 Extension 机制支持业务系统扩展。所有业务能力（如群组、好友、工作流等）都通过 Extension 接入，确保 Core SDK 保持精简和稳定。

**核心原则**：
- ✅ Core SDK 只包含核心功能（Session、Connection、Message、Conversation、Sync）
- ✅ 所有业务能力通过 Extension 机制接入
- ✅ Extension 必须复用 Core Sync 能力，不允许绕过 Sync Engine
- ✅ Extension 通过 SdkContext 注册，遵循 DDD + CQRS 架构

## 扩展模块架构

```
flare-im-core-sdk/
├── src/
│   ├── extensions/              # 扩展模块基础包
│   │   ├── mod.rs               # 扩展模块导出
│   │   ├── group/               # 群组扩展（示例）
│   │   │   └── mod.rs
│   │   ├── friend/              # 好友扩展（示例）
│   │   │   └── mod.rs
│   │   └── README.md            # 本文档
│   └── application/
│       └── extension/           # 扩展机制核心
│           └── mod.rs           # ExtensionPoint trait
```

## 扩展模块开发步骤

### 1. 创建扩展模块目录

在 `src/extensions/` 下创建你的扩展模块目录：

```bash
mkdir -p src/extensions/your_module
```

### 2. 定义扩展模块结构

创建 `src/extensions/your_module/mod.rs`：

```rust
//! 你的业务模块扩展
//!
//! 职责：实现业务特定的功能，通过 Extension 机制接入 SDK

use crate::application::extension::{SdkExtension, SdkContext, SyncSpec, ExtensionSyncMode};
use anyhow::Result;
use std::sync::Arc;

/// 你的业务模块扩展
pub struct YourModuleExtension {
    // 扩展模块的依赖
    // 例如：数据库连接、缓存、网络客户端等
}

impl YourModuleExtension {
    /// 创建新的扩展实例
    pub fn new() -> Self {
        Self {
            // 初始化依赖
        }
    }
}

/// 实现 SdkExtension trait
impl SdkExtension for YourModuleExtension {
    /// 扩展名称
    fn name(&self) -> &'static str {
        "your_module"
    }
    
    /// 注册 Extension（注入到 SDK Context）
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 注册你的业务模块相关的命令处理器、查询处理器等
        // 可以通过 ctx 访问核心能力：
        // - ctx.command_handler: 注册命令处理器
        // - ctx.query_handler: 注册查询处理器
        // - ctx.event_bus: 订阅/发布事件
        // - ctx.event_store: 持久化领域事件
        // - ctx.read_store: 查询数据
        // - ctx.sync_coordinator: 执行同步
        
        // TODO: 实现你的业务逻辑注册
        Ok(())
    }
    
    /// 返回 Extension 的同步规格
    fn sync_specs(&self) -> Vec<SyncSpec> {
        vec![
            SyncSpec::with_priority(
                "your_module_list".to_string(),
                ExtensionSyncMode::Bootstrap, // 需要在 Bootstrap 时同步
                10, // 高优先级
            ),
            SyncSpec::new(
                "your_module_data".to_string(),
                ExtensionSyncMode::Async, // 异步同步
            ),
        ]
    }
}

impl YourModuleExtension {
    /// 你的业务方法
    pub fn your_business_method(&self) -> Result<()> {
        // 实现业务逻辑
        Ok(())
    }
}

impl Default for YourModuleExtension {
    fn default() -> Self {
        Self::new()
    }
}
```

### 3. 注册扩展模块

在 `src/extensions/mod.rs` 中注册你的扩展：

```rust
//! 扩展模块
//!
//! 所有业务扩展都在这里注册

pub mod group;
pub mod friend;
pub mod your_module;  // 添加你的模块

pub use group::GroupExtension;
pub use friend::FriendExtension;
pub use your_module::YourModuleExtension;  // 导出你的扩展
```

### 4. 在 SDK 中启用扩展

在 `src/lib.rs` 中启用扩展功能（如果使用 feature flag）：

```rust
// Extension 模块（可选）
#[cfg(feature = "extensions")]
pub mod extensions {
    pub mod group;
    pub mod friend;
    pub mod your_module;  // 添加你的模块
}
```

### 5. 在 SDK 初始化时注册扩展

在创建 SDK 实例时注册扩展（通常在 `ImCoreSdk::new` 中）：

```rust
use crate::extensions::YourModuleExtension;
use crate::application::extension::SdkExtension;

// 在 SDK 初始化后注册扩展
let sdk = ImCoreSdk::new(config).await?;

// 创建并注册扩展
let your_module_extension = Arc::new(YourModuleExtension::new());
sdk.register_extension(your_module_extension).await?;

// 同步规格会自动注册到 SyncCoordinator
// Bootstrap Sync 会在 bootstrap_sync() 时自动执行
// Async Sync 可以通过 sync_all_extensions() 执行
```

## 扩展模块开发规范

### 1. 目录结构

```
your_module/
├── mod.rs                    # 模块入口
├── domain/                   # 领域层（可选）
│   ├── model.rs              # 领域模型
│   ├── service.rs            # 领域服务
│   └── mod.rs
├── application/              # 应用层（可选）
│   ├── command.rs            # 命令处理
│   ├── query.rs              # 查询处理
│   └── mod.rs
├── infrastructure/           # 基础设施层（可选）
│   ├── repository.rs         # 仓储实现
│   └── mod.rs
└── interface/               # 接口层（可选）
    ├── facade.rs             # Facade API
    └── mod.rs
```

### 2. 命名规范

- **模块名**: `snake_case` (例如: `your_module`)
- **类型名**: `PascalCase` (例如: `YourModuleExtension`)
- **函数名**: `snake_case` (例如: `handle_your_event`)

### 3. 依赖管理

扩展模块应该：
- ✅ **最小依赖**: 只依赖必要的 crate
- ✅ **避免循环依赖**: 不依赖其他扩展模块
- ✅ **使用 Core SDK API**: 通过 Facade 访问 Core 功能

### 4. 事件处理

扩展模块可以通过在 `register` 方法中注册事件监听器来处理领域事件：

```rust
impl SdkExtension for YourModuleExtension {
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 订阅事件
        let mut receiver = ctx.event_bus.subscribe();
        let extension_clone = Arc::new(self.clone()); // 假设实现了 Clone
        tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                // 处理事件
                if event.event_type == "Message.Sent" {
                    // extension_clone.on_message_sent(&event).await?;
                }
            }
        });
        
        Ok(())
    }
}

impl YourModuleExtension {
    fn on_message_sent(&self, event: &DomainEvent) -> Result<()> {
        // 处理消息发送事件
        Ok(())
    }
}
```

### 5. 状态管理

扩展模块应该：
- ✅ **无状态设计**: 尽量保持无状态
- ✅ **使用 EventStore**: 通过事件溯源管理状态
- ✅ **使用 ReadStore**: 通过读模型查询状态

### 6. 错误处理

```rust
use anyhow::{Result, Context};

impl YourModuleExtension {
    pub fn your_method(&self) -> Result<()> {
        // 使用 anyhow::Context 提供错误上下文
        self.do_something()
            .context("Failed to do something")?;
        Ok(())
    }
}
```

## 扩展模块示例

### 示例 1: 群组扩展

参考 `src/extensions/group/mod.rs`：

```rust
//! 群组扩展
//!
//! 实现群组相关的业务功能

use crate::application::extension::{SdkExtension, SdkContext, SyncSpec, ExtensionSyncMode};

pub struct GroupExtension;

impl SdkExtension for GroupExtension {
    fn name(&self) -> &'static str {
        "group"
    }
    
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 注册群组相关的命令处理器、查询处理器等
        // TODO: 实现群组相关的业务逻辑
        Ok(())
    }
    
    fn sync_specs(&self) -> Vec<SyncSpec> {
        vec![
            SyncSpec::with_priority(
                "group_list".to_string(),
                ExtensionSyncMode::Bootstrap, // 群组列表需要在 Bootstrap 时同步
                10, // 高优先级
            ),
            SyncSpec::new(
                "group_members".to_string(),
                ExtensionSyncMode::Async, // 群组成员异步同步
            ),
        ]
    }
}
```

### 示例 2: 好友扩展

参考 `src/extensions/friend/mod.rs`：

```rust
//! 好友扩展
//!
//! 实现好友相关的业务功能

use crate::application::extension::{SdkExtension, SdkContext, SyncSpec, ExtensionSyncMode};

pub struct FriendExtension;

impl SdkExtension for FriendExtension {
    fn name(&self) -> &'static str {
        "friend"
    }
    
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 注册好友相关的命令处理器、查询处理器等
        // TODO: 实现好友相关的业务逻辑
        Ok(())
    }
    
    fn sync_specs(&self) -> Vec<SyncSpec> {
        vec![
            SyncSpec::with_priority(
                "friend_list".to_string(),
                ExtensionSyncMode::Bootstrap, // 好友列表需要在 Bootstrap 时同步
                10, // 高优先级
            ),
            SyncSpec::new(
                "friend_status".to_string(),
                ExtensionSyncMode::Async, // 好友状态异步同步
            ),
        ]
    }
}
```

## 扩展模块最佳实践

### 1. 单一职责

每个扩展模块只负责一个业务领域：
- ✅ `group` - 群组功能
- ✅ `friend` - 好友功能
- ✅ `workflow` - 工作流功能
- ❌ `business` - 太宽泛，应该拆分

### 2. 独立测试

为每个扩展模块编写独立的测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_your_module() {
        let extension = YourModuleExtension::new();
        assert_eq!(extension.name(), "your_module");
    }
}
```

### 3. 文档完善

为扩展模块提供完整的文档：
- 模块职责说明
- API 文档
- 使用示例
- 配置说明

### 4. 版本管理

扩展模块应该：
- ✅ 遵循语义化版本
- ✅ 提供版本兼容性说明
- ✅ 支持多版本共存（如果必要）

### 5. 配置管理

扩展模块的配置应该：
- ✅ 通过 `SdkConfig` 传递
- ✅ 支持环境变量覆盖
- ✅ 提供默认配置

## 扩展模块与 Core SDK 的交互

### 1. 访问 Core SDK API

扩展模块通过 Facade 访问 Core SDK 功能：

```rust
use crate::interface::facade::ImCoreSdk;

impl YourModuleExtension {
    pub async fn use_core_api(&self, sdk: &ImCoreSdk) -> Result<()> {
        // 使用 Core SDK 的消息 API
        let message = sdk.message().create_text_message(
            "conv_123".to_string(),
            "user_123".to_string(),
            "Hello".to_string(),
            tenant,
        )?;
        
        // 发送消息
        sdk.message().send_message(message).await?;
        
        Ok(())
    }
}
```

### 2. 发布领域事件

扩展模块可以发布领域事件：

```rust
use crate::domain::event::DomainEvent;

impl YourModuleExtension {
    pub async fn publish_event(&self, event_store: &dyn EventStore) -> Result<()> {
        let event = DomainEvent::new(
            "YourModule.YourEvent",
            "aggregate_id",
            1,
            serde_json::json!({
                "data": "your_data"
            }),
        );
        event_store.append(event).await?;
        Ok(())
    }
}
```

### 3. 订阅领域事件

扩展模块通过 `SdkContext::event_bus` 订阅事件：

```rust
impl SdkExtension for YourModuleExtension {
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 订阅事件
        let mut receiver = ctx.event_bus.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = receiver.recv().await {
                // 处理事件
                match event.event_type.as_str() {
                    "Message.Sent" => {
                        // 处理消息发送事件
                    }
                    _ => {}
                }
            }
        });
        Ok(())
    }
}
```

## 扩展模块开发检查清单

### 开发前准备

- [ ] 确定扩展模块的职责和边界
- [ ] 设计扩展模块的领域模型
- [ ] 确定需要订阅的领域事件
- [ ] 确定需要发布的领域事件
- [ ] 设计扩展模块的 API
- [ ] 确定同步需求（Bootstrap/Async）

### 开发过程

- [ ] 创建扩展模块目录结构
- [ ] 实现 `SdkExtension` trait
  - [ ] 实现 `name()` 方法
  - [ ] 实现 `register()` 方法
  - [ ] 实现 `sync_specs()` 方法
- [ ] 实现领域模型（如果需要）
- [ ] 实现领域服务（如果需要）
- [ ] 实现应用层（如果需要）
- [ ] 实现基础设施层（如果需要）
- [ ] 实现接口层（如果需要）
- [ ] 编写单元测试
- [ ] 编写集成测试

### 开发后检查

- [ ] 代码通过 `cargo check`
- [ ] 代码通过 `cargo clippy`
- [ ] 代码通过 `cargo fmt --check`
- [ ] 所有测试通过
- [ ] 文档完整
- [ ] 示例代码可用
- [ ] 在 `extensions/mod.rs` 中注册
- [ ] 在 SDK 初始化时注册扩展

## 常见问题

### Q1: 扩展模块可以访问 Core SDK 的内部实现吗？

**A**: 不建议。扩展模块应该通过 Facade API 访问 Core SDK 功能，保持解耦。

### Q2: 扩展模块可以依赖其他扩展模块吗？

**A**: 不建议。扩展模块应该相互独立，避免循环依赖。如果确实需要，应该通过事件机制通信。或者将公共功能提取到 Core SDK 中。

### Q3: 扩展模块可以修改 Core SDK 的状态吗？

**A**: 不可以。扩展模块只能通过 Core SDK 的 API（Facade）操作状态，不能直接修改 Core SDK 的内部状态。所有状态变更必须通过 FSM 和 EventStore。

### Q4: 扩展模块如何持久化数据？

**A**: 扩展模块应该：
1. 通过 `EventStore` 发布领域事件（推荐）
2. 通过 `ReadStore` 查询读模型
3. 如果需要自定义存储，实现自己的 Repository（遵循 DDD 原则）
4. **禁止绕过 Sync Engine**：所有数据同步必须通过统一的同步引擎

### Q5: 扩展模块如何与 UI 层交互？

**A**: 扩展模块可以：
1. 通过 Facade 暴露 API（推荐）
2. 通过 EventBus 发布事件供 UI 订阅
3. 通过回调函数通知 UI（不推荐，耦合度高）

### Q6: 扩展模块如何定义同步需求？

**A**: 通过 `sync_specs()` 方法返回 `Vec<SyncSpec>`：
- `Bootstrap` 模式：必须在 SDK Ready 前完成，失败则 SDK 不可用
- `Async` 模式：在后台执行，可以失败和重试

### Q7: 扩展模块需要实现哪些必需的方法？

**A**: 必须实现 `SdkExtension` trait 的三个方法：
1. `name()` - 返回扩展名称
2. `register()` - 注册扩展到 SdkContext
3. `sync_specs()` - 返回同步规格列表

## 参考资源

- [Core SDK 架构文档](../doc/IM_Core_SDK_Architecture.md)
- [领域驱动设计指南](../doc/DDD_GUIDE.md)
- [事件溯源指南](../doc/EVENT_SOURCING_GUIDE.md)
- [扩展机制实现](../src/application/extension/mod.rs)

## 总结

扩展模块是 Flare IM Core SDK 的核心设计之一，通过 Extension 机制，业务系统可以：

1. ✅ **保持 Core 精简**: Core SDK 只包含核心功能
2. ✅ **灵活扩展**: 业务系统可以按需扩展功能
3. ✅ **解耦设计**: 扩展模块与 Core SDK 解耦
4. ✅ **统一同步**: 所有扩展都通过统一的同步引擎

遵循本指南，你可以轻松开发出符合架构规范的扩展模块！
