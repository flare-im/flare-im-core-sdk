# Flare IM Core SDK 重构计划

## 重构目标

1. **简化代码结构**：移除冗余封装，直接使用 `flare-proto`
2. **专注核心功能**：仅包含会话（Session）和消息（Message）两个核心模块
3. **建立扩展机制**：为后续扩展用户、好友、群等模块做准备
4. **保持代码简洁**：遵循 Rust 最佳实践，代码清晰易读

## 重构步骤

### 阶段 1：简化 Model 模块 ✅

**目标**：直接使用 `flare-proto`，移除不必要的封装

**变更**：
- `model/message.rs`: 直接 re-export `flare_proto::Message` 和相关类型
- `model/session.rs`: 简化 `SessionSummary`，只保留必要的转换逻辑
- `model/message_builder.rs`: 保留，但简化实现
- `model/sync.rs`: 保留，但简化 `SyncCursor` 结构

### 阶段 2：简化 Service 模块

**目标**：专注于核心功能，移除冗余代码

**变更**：
- `service/message/service.rs`: 简化消息发送逻辑
- `service/session.rs`: 简化会话查询逻辑
- `service/sync.rs`: 简化同步逻辑

### 阶段 3：简化 Storage 模块

**目标**：保持接口简洁，专注于会话和消息存储

**变更**：
- `storage/storage_trait.rs`: 保持现有接口，但简化文档
- 移除不必要的存储操作

### 阶段 4：建立扩展系统

**目标**：为后续扩展做准备

**变更**：
- `extension/`: 建立扩展注册表和钩子机制
- 支持会话扩展和消息扩展

### 阶段 5：简化 Client 模块

**目标**：整合所有模块，提供简洁的 API

**变更**：
- `client.rs`: 简化客户端主入口
- 移除不必要的中间层

## 代码规范

1. **直接使用 flare-proto**：不进行二次封装，避免转换开销
2. **最小化抽象**：只在必要时创建 trait 和抽象层
3. **清晰的错误处理**：使用 `anyhow::Result` 统一错误处理
4. **完整的文档**：所有公共 API 都有文档注释

