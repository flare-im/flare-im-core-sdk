# 统一 C ABI 绑定层 - 后续完善说明

## 当前状态

核心框架已完成,但存在一些编译错误需要根据实际 SDK API 进行调整。

## 主要问题

### 1. API 方法签名不匹配

部分 SDK API 方法签名与实现不匹配,需要根据实际 SDK 定义调整:

- **消息 API**: `message_build()`, `message()` 等方法可能返回 `Option` 而不是 `Result`
- **会话 API**: `conversation()` 方法签名需要确认
- **媒体 API**: `media()` 方法签名需要确认

### 2. 类型序列化问题

- `SendAck` 等类型可能没有实现 `Serialize` trait
- 需要为这些类型实现自定义序列化或使用其他方式传递数据

### 3. 存储提供者

- `LoginDbKind::Sqlite` 需要 `lifecycle-sqlite` feature
- `LoginDbKind::IndexedDb` 需要 `StoreProvider` 参数
- 需要在 FFI 层提供存储提供者的创建方式

## 解决方案

### 短期方案

1. **简化实现**: 暂时注释掉有问题的 API 实现,保留核心框架
2. **使用占位符**: 对于无法立即实现的功能,使用 TODO 注释标记
3. **文档说明**: 在文档中说明哪些功能暂未实现

### 长期方案

1. **API 适配层**: 创建适配层处理 SDK API 的不同返回类型
2. **自定义序列化**: 为 SDK 类型实现自定义 JSON 序列化
3. **存储抽象**: 在 FFI 层提供存储配置的 JSON 接口

## 建议的完善步骤

### 第一步: 确认 SDK API

```bash
# 查看 SDK 公共 API
cd flare-im-core-sdk
cargo doc --open
```

### 第二步: 创建 API 适配器

```rust
// bindings/c/src/adapter.rs
pub fn get_message_api(client: &IMClient) -> Result<&MessageApi, FlareErrorCode> {
    client.message().ok_or(FlareErrorCode::NotConnected)
}
```

### 第三步: 实现自定义序列化

```rust
// 为 SendAck 等类型实现序列化
impl Serialize for SendAckWrapper {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        // 自定义序列化逻辑
    }
}
```

### 第四步: 完善存储配置

```rust
// 提供存储配置的 JSON 接口
pub fn create_store_provider(config_json: &str) -> Result<StoreProvider, FlareErrorCode> {
    // 解析配置并创建存储提供者
}
```

## 当前可用的功能

以下功能已实现且应该可以工作:

1. ✅ SDK 实例创建和销毁 (`flare_im_new`, `flare_im_free`)
2. ✅ SDK 初始化 (`flare_im_init`)
3. ✅ 状态查询 (`flare_im_is_connected`, `flare_im_state`)
4. ✅ 事件订阅 (`flare_im_subscribe_events`, `flare_im_unsubscribe`)
5. ✅ 字符串内存管理 (`flare_im_string_free`, `flare_im_bytes_free`)

## 需要进一步实现的功能

1. ⚠️ 登录/登出 (需要存储提供者配置)
2. ⚠️ 消息 API (需要确认 API 签名)
3. ⚠️ 会话 API (需要确认 API 签名)
4. ⚠️ 媒体 API (需要确认 API 签名)

## 测试建议

1. 先测试基础功能 (创建、销毁、状态查询)
2. 再测试事件订阅
3. 最后测试完整的消息收发流程

## 平台适配层

在核心 FFI 层完善后,可以实现:

1. Flutter Dart 适配层 (~200 LOC)
2. Android Kotlin 适配层 (~300 LOC)
3. iOS Swift 适配层 (~200 LOC)

## 总结

核心框架已搭建完成,架构设计合理。后续需要:
1. 根据实际 SDK API 调整实现细节
2. 解决类型序列化问题
3. 完善存储配置接口
4. 实现平台适配层

建议先让核心框架编译通过,再逐步完善各个 API 的实现。
