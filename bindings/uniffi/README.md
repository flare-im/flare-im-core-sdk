# Flare IM Core SDK - UniFFI Bindings

UniFFI 绑定层，为 Flare IM Core SDK 提供跨语言绑定支持（Kotlin、Swift 等）。

## 概述

此模块使用 [UniFFI](https://mozilla.github.io/uniffi-rs/) 自动生成 Kotlin、Swift 等语言的绑定，使 Flare IM Core SDK 可以在移动平台（Android、iOS）上使用。

## 架构设计

```
┌─────────────────────────────────────────────────────────┐
│   Kotlin/Swift 应用层                                    │
└──────────────────┬──────────────────────────────────────┘
                   │ 使用生成的绑定
┌──────────────────▼──────────────────────────────────────┐
│   UniFFI 绑定层 (bindings/uniffi)                       │
│   - im.udl: 接口定义                                     │
│   - lib.rs: Rust 实现                                   │
└──────────────────┬──────────────────────────────────────┘
                   │ 调用
┌──────────────────▼──────────────────────────────────────┐
│   Flare IM Core SDK (core)                              │
│   - interface/facade: Rust 原生 API                     │
│   - domain: 领域模型                                     │
│   - application: 应用服务                                │
└─────────────────────────────────────────────────────────┘
```

## 特性

- ✅ **完整的 API 支持**: 覆盖消息、会话、事件订阅等核心功能
- ✅ **事件回调**: 支持消息、连接、会话等事件的回调
- ✅ **类型安全**: 使用 UniFFI 的类型系统确保类型安全
- ✅ **自动生成**: 从 UDL 文件自动生成各语言绑定

## 构建

### 前置要求

- Rust 1.85+ (Edition 2024)
- UniFFI CLI: `cargo install uniffi_bindgen`

### 构建 Rust 库

```bash
cd flare-im-core-sdk/bindings/uniffi
cargo build --release
```

### 生成绑定

```bash
# 生成 Kotlin 绑定
uniffi-bindgen generate src/im.udl --language kotlin --out-dir ../../generated/kotlin

# 生成 Swift 绑定
uniffi-bindgen generate src/im.udl --language swift --out-dir ../../generated/swift
```

## API 文档

### SDK 初始化

```rust
let config = SdkConfig {
    websocket_url: Some("ws://localhost:8080".to_string()),
    quic_url: None,
    storage_path: Some("/data/flare_im".to_string()),
    media_cache_path: None,
    log_level: "info".to_string(),
};

let sdk = ImCoreSdk::new(config)?;
```

### 生命周期管理

```rust
// 登录
sdk.login("user_id".to_string(), "token".to_string())?;

// 连接
sdk.connect()?;

// 同步
sdk.bootstrap_sync()?;

// 登出
sdk.logout()?;
```

### 消息操作

```rust
// 创建文本消息
let message = sdk.create_text_message(
    "conversation_id".to_string(),
    "sender_id".to_string(),
    "Hello, World!".to_string(),
    TenantContext {
        tenant_id: "tenant1".to_string(),
        user_id: "user1".to_string(),
    },
    Some("receiver_id".to_string()),
)?;

// 发送消息
sdk.send_message(message)?;

// 获取消息列表
let messages = sdk.get_messages("conversation_id".to_string(), Some(20))?;

// 撤回消息
sdk.revoke_message("conversation_id".to_string(), "message_id".to_string(), "user_id".to_string())?;
```

### 会话操作

```rust
// 获取所有会话
let conversations = sdk.get_all_conversations()?;

// 获取单个会话
let conversation = sdk.get_conversation("conversation_id".to_string())?;

// 标记会话已读
sdk.mark_conversation_read("conversation_id".to_string(), "user_id".to_string())?;

// 获取总未读数
let unread_count = sdk.get_total_unread_count()?;
```

### 事件订阅

```rust
// 创建事件订阅者
struct MyMessageSubscriber;

impl MessageEventSubscriber for MyMessageSubscriber {
    fn on_message_created(&self, message: Message) {
        println!("Message created: {}", message.message_id);
    }
    
    fn on_message_sent(&self, message: Message) {
        println!("Message sent: {}", message.message_id);
    }
    
    // ... 实现其他回调方法
}

// 订阅消息事件
let subscriber = Arc::new(MyMessageSubscriber);
let subscriber_id = sdk.subscribe_message_events(subscriber)?;

// 取消订阅
sdk.unsubscribe_message_events(subscriber_id)?;
```

## 事件类型

### MessageEventSubscriber

- `on_message_created`: 消息已创建
- `on_message_sent`: 消息已发送
- `on_message_send_failed`: 消息发送失败
- `on_message_delivered`: 消息已送达
- `on_message_read`: 消息已读
- `on_message_recalled`: 消息已撤回
- `on_message_edited`: 消息已编辑
- `on_message_deleted`: 消息已删除
- `on_reaction_added`: 反应已添加
- `on_reaction_removed`: 反应已移除

### ConnectionEventSubscriber

- `on_connected`: 连接已建立
- `on_disconnected`: 连接已断开
- `on_reconnecting`: 正在重连
- `on_reconnected`: 重连成功
- `on_connect_failed`: 连接失败

### ConversationEventSubscriber

- `on_conversation_created`: 会话已创建
- `on_unread_updated`: 未读数已更新
- `on_last_message_updated`: 最后一条消息已更新
- `on_marked_as_read`: 会话已标记为已读
- `on_draft_updated`: 草稿已更新
- `on_hidden`: 会话已隐藏
- `on_deleted`: 会话已删除
- `on_messages_cleared`: 会话消息已清空
- `on_updated`: 会话信息已更新
- `on_input_state_updated`: 输入状态已更新

## 限制与注意事项

1. **异步操作**: UniFFI 绑定层使用 `Runtime::block_on` 将异步操作转换为同步调用，这意味着所有操作都会阻塞当前线程。在生产环境中，建议在后台线程中调用这些方法。

2. **事件回调**: 事件回调在 Rust 的异步运行时中执行，可能不在主线程中。在移动平台上，需要手动切换到主线程更新 UI。

3. **类型转换**: 部分复杂类型（如 `DateTime<Utc>`）被转换为字符串（ISO 8601 格式），需要在客户端进行解析。

4. **错误处理**: 所有错误都通过 `SdkError` 返回，客户端需要处理所有可能的错误类型。

## 开发指南

### 添加新的 API

1. 在 `im.udl` 中添加接口定义
2. 在 `lib.rs` 中实现对应的 Rust 方法
3. 重新生成绑定: `uniffi-bindgen generate src/im.udl --language <language>`

### 添加新的事件类型

1. 在 `im.udl` 中添加回调接口定义
2. 在 `lib.rs` 中实现 `trait` 和包装器
3. 实现事件订阅方法
4. 重新生成绑定

## 参考

- [UniFFI 文档](https://mozilla.github.io/uniffi-rs/)
- [Flare IM Core SDK 文档](../README.md)
- [Interface API 文档](../src/interface/README.md)
