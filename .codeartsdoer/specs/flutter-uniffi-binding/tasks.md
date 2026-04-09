# 统一 C ABI 绑定层任务规划

## 任务概览

| 类别 | 主任务数 | 子任务数 | 预估工时 |
|------|---------|---------|---------|
| C ABI 核心实现 | 6 | 18 | 8 人日 |
| 平台适配层 | 3 | 9 | 4 人日 |
| 构建与测试 | 2 | 6 | 2 人日 |
| 文档与示例 | 1 | 3 | 1 人日 |
| **合计** | **12** | **36** | **15 人日** |

---

## 任务 1: C ABI 基础设施搭建

### 1.1 创建错误码模块

**描述**: 定义 C ABI 错误码枚举，实现从 SDK 错误到 C 错误码的映射

**输入**:
- `flare-im-core-sdk/src/error.rs` 中的 `ErrorCode` 定义

**输出**:
- `bindings/c/src/error.rs` 包含 `FlareErrorCode` 枚举和映射实现

**验收条件**:
- [ ] `FlareErrorCode` 枚举包含所有错误分类（连接、参数、网络、认证、存储、内部）
- [ ] 实现 `From<ErrorCode> for FlareErrorCode`
- [ ] 实现 `From<&FlareError> for FlareErrorCode`
- [ ] 错误码值与 spec.md 定义一致

**代码生成提示**:
```rust
// bindings/c/src/error.rs
// 定义 FlareErrorCode 枚举，使用 #[repr(C, i32)]
// 实现 From trait 从 flare_im_core_sdk::ErrorCode 映射
// 实现 From trait 从 &flare_im_core_sdk::FlareError 映射
```

### 1.2 创建句柄管理模块

**描述**: 实现句柄类型定义、全局句柄表、句柄生命周期管理

**输入**:
- `flare-im-core-sdk::client::IMClient` 类型

**输出**:
- `bindings/c/src/handle.rs` 包含句柄类型和管理函数

**验收条件**:
- [ ] `FlareImHandle` 和 `FlareEventSubscription` 类型定义
- [ ] 全局句柄表 `HANDLE_TABLE` 使用 `RwLock<HashMap>`
- [ ] `get_instance` 函数从句柄获取 SDK 实例
- [ ] 线程安全的句柄 ID 生成

**代码生成提示**:
```rust
// bindings/c/src/handle.rs
// 使用 lazy_static! 定义 HANDLE_TABLE
// 使用 AtomicU64 生成唯一句柄 ID
// 定义 SdkInstance 结构体包含 client、runtime、subscriptions
```

### 1.3 创建回调管理模块

**描述**: 定义回调类型、实现回调上下文包装和调度

**输入**:
- C ABI 回调签名规范

**输出**:
- `bindings/c/src/callback.rs` 包含回调类型和调度函数

**验收条件**:
- [ ] 定义 `FlareResultCallback`、`FlareJsonCallback`、`FlareEventCallback` 等类型
- [ ] `CallbackContext` 结构体包装用户上下文和回调
- [ ] `invoke_result_callback`、`invoke_json_callback` 等调度函数
- [ ] 回调上下文实现 `Send` 和 `Sync`

**代码生成提示**:
```rust
// bindings/c/src/callback.rs
// 使用 extern "C" fn 定义回调类型
// 实现 CallbackContext<C> 泛型结构体
// unsafe impl Send/Sync for CallbackContext
```

### 1.4 创建字符串内存管理模块

**描述**: 实现跨语言字符串传递的内存管理

**输入**:
- Rust `String` 和 C 字符串互转需求

**输出**:
- `bindings/c/src/string.rs` 包含内存管理函数

**验收条件**:
- [ ] `flare_im_string_free` 函数释放 C 字符串
- [ ] `flare_im_bytes_free` 函数释放字节数组
- [ ] `string_to_c` 辅助函数将 Rust String 转为 C 字符串
- [ ] `parse_string` 辅助函数解析 C 字符串为 Rust String

**代码生成提示**:
```rust
// bindings/c/src/string.rs
// 使用 CString::into_raw 和 CString::from_raw 管理内存
// 实现 #[no_mangle] extern "C" 函数
```

---

## 任务 2: 生命周期 API 实现

### 2.1 实现 SDK 创建与销毁

**描述**: 实现 `flare_im_new` 和 `flare_im_free` 函数

**输入**:
- `IMClient::new()` 构造函数
- 句柄管理模块

**输出**:
- `bindings/c/src/lifecycle.rs` 中的创建和销毁函数

**验收条件**:
- [ ] `flare_im_new` 创建新句柄并注册到全局表
- [ ] `flare_im_free` 从全局表移除句柄并释放资源
- [ ] `flare_im_free` 取消所有事件订阅
- [ ] 使用 `catch_panic` 包装防止异常跨越 FFI

**代码生成提示**:
```rust
// bindings/c/src/lifecycle.rs
// flare_im_new: 创建 IMClient，生成句柄，注册到 HANDLE_TABLE
// flare_im_free: 取消订阅，从 HANDLE_TABLE 移除，drop 实例
```

### 2.2 实现 SDK 初始化

**描述**: 实现 `flare_im_init` 异步初始化函数

**输入**:
- `IMClient::init` 方法
- `SdkConfigOverlay` 配置类型

**输出**:
- `bindings/c/src/lifecycle.rs` 中的 `flare_im_init` 函数

**验收条件**:
- [ ] 解析 JSON 配置为 `SdkConfigOverlay`
- [ ] 在 Tokio runtime 中执行异步初始化
- [ ] 通过回调返回结果
- [ ] 错误时返回正确的错误码

**代码生成提示**:
```rust
// bindings/c/src/lifecycle.rs
// 解析 config_json 为 SdkConfigOverlay
// 使用 instance.runtime.spawn 执行异步操作
// 调用 client.init(None, Some(config)).await
// 通过回调返回结果
```

### 2.3 实现登录与登出

**描述**: 实现 `flare_im_login` 和 `flare_im_logout` 函数

**输入**:
- `IMClient::login` 和 `IMClient::logout` 方法

**输出**:
- `bindings/c/src/lifecycle.rs` 中的登录登出函数

**验收条件**:
- [ ] `flare_im_login` 传入 user_id、token 和回调
- [ ] 登录成功后可接收事件
- [ ] `flare_im_logout` 断开连接并清理状态
- [ ] 使用 `LoginDbKind::Sqlite` 作为存储类型

**代码生成提示**:
```rust
// bindings/c/src/lifecycle.rs
// flare_im_login: 调用 client.login(&user_id, Some(&token), LoginDbKind::Sqlite, |bus, _| {})
// flare_im_logout: 调用 client.logout().await
```

### 2.4 实现状态查询

**描述**: 实现同步状态查询函数

**输入**:
- `IMClient::is_connected`、`IMClient::current_user_id`、`IMClient::state` 方法

**输出**:
- `bindings/c/src/lifecycle.rs` 中的状态查询函数

**验收条件**:
- [ ] `flare_im_is_connected` 返回 bool
- [ ] `flare_im_current_user_id` 返回字符串（需释放）
- [ ] `flare_im_state` 返回状态字符串
- [ ] 无效句柄返回安全默认值

**代码生成提示**:
```rust
// bindings/c/src/lifecycle.rs
// flare_im_is_connected: 检查 client.state() == SdkState::Ready
// flare_im_current_user_id: 调用 client.current_user_id().await（需阻塞版本或缓存）
// flare_im_state: 返回 format!("{:?}", client.state())
```

---

## 任务 3: 事件订阅 API 实现

### 3.1 实现事件类型映射

**描述**: 实现 `SdkEvent` 到事件类型字符串和 JSON 的映射

**输入**:
- `flare-im-core-sdk::event::SdkEvent` 类型定义

**输出**:
- `bindings/c/src/event.rs` 中的事件映射函数

**验收条件**:
- [ ] `event_type_to_string` 返回事件类型字符串
- [ ] `event_to_json` 返回事件 JSON 字符串
- [ ] 覆盖所有事件类型（Connection、Message、Conversation、Sync）
- [ ] JSON 格式与 spec.md 定义一致

**代码生成提示**:
```rust
// bindings/c/src/event.rs
// 定义 EventPayload 结构体用于 JSON 序列化
// 实现 event_type_to_string: match event { SdkEvent::Connection(ConnectionEvent::Connected) => "connection.connected", ... }
// 实现 event_to_json: serde_json::to_string(&EventPayload::from(event))
```

### 3.2 实现事件订阅

**描述**: 实现 `flare_im_subscribe_events` 和 `flare_im_unsubscribe` 函数

**输入**:
- `IMClient::bus` 方法获取 EventBus
- `EventBus::subscribe` 订阅事件

**输出**:
- `bindings/c/src/event.rs` 中的订阅函数

**验收条件**:
- [ ] `flare_im_subscribe_events` 返回订阅句柄
- [ ] 启动异步任务转发事件到回调
- [ ] `flare_im_unsubscribe` 取消订阅并释放资源
- [ ] 支持多个并发订阅

**代码生成提示**:
```rust
// bindings/c/src/event.rs
// 定义 EventSubscriptionInner 包含 id、callback、user_context、cancel_tx
// spawn_event_forwarder: 订阅 EventBus，循环接收事件，调用回调
// 使用 tokio::select! 处理取消信号
```

### 3.3 实现事件转发任务

**描述**: 实现从 EventBus 到 C 回调的事件转发

**输入**:
- EventBus 订阅接收器
- 事件回调函数

**输出**:
- `bindings/c/src/event.rs` 中的 `spawn_event_forwarder` 函数

**验收条件**:
- [ ] 在 Tokio runtime 中异步执行
- [ ] 正确处理 `Lagged` 和 `Closed` 错误
- [ ] 支持通过 `oneshot::channel` 取消
- [ ] 事件 JSON 内存由调用方释放

**代码生成提示**:
```rust
// bindings/c/src/event.rs
// instance.runtime.spawn(async move { loop { tokio::select! { ... } } })
// 处理 broadcast::error::RecvError::Lagged 和 Closed
// 调用 callback(user_context, event_type, event_json)
```

---

## 任务 4: 消息 API 实现

### 4.1 实现消息构建函数

**描述**: 实现各类消息构建函数（文本、图片、文件等）

**输入**:
- `MessageBuildApi` 的 `create_text`、`create_image`、`create_file` 等方法

**输出**:
- `bindings/c/src/message.rs` 中的消息构建函数

**验收条件**:
- [ ] `flare_im_create_text_message` 创建文本消息
- [ ] `flare_im_create_image_message` 创建图片消息
- [ ] `flare_im_create_file_message` 创建文件消息
- [ ] 返回消息 JSON 字符串

**代码生成提示**:
```rust
// bindings/c/src/message.rs
// 获取 client.message_build()
// 调用 build_api.create_text(&conversation_id, &text).await
// 序列化消息为 JSON 返回
```

### 4.2 实现消息发送函数

**描述**: 实现 `flare_im_send_message` 和 `flare_im_send_media_message` 函数

**输入**:
- `MessageApi::send` 方法

**输出**:
- `bindings/c/src/message.rs` 中的发送函数

**验收条件**:
- [ ] `flare_im_send_message` 发送消息并返回 `client_msg_id`
- [ ] `flare_im_send_media_message` 支持进度回调
- [ ] 消息 JSON 解析为 `IMMessage`
- [ ] 发送结果通过回调返回

**代码生成提示**:
```rust
// bindings/c/src/message.rs
// 解析 message_json 为 IMMessage
// 调用 api.send(message).await
// 返回 client_msg_id
```

### 4.3 实现消息查询函数

**描述**: 实现消息查询函数（获取、列表、搜索）

**输入**:
- `MessageApi::get`、`MessageApi::list`、`MessageApi::search` 方法

**输出**:
- `bindings/c/src/message.rs` 中的查询函数

**验收条件**:
- [ ] `flare_im_get_message` 获取单条消息
- [ ] `flare_im_list_messages` 获取消息列表
- [ ] `flare_im_search_messages` 搜索消息
- [ ] 返回消息 JSON 或 JSON 数组

**代码生成提示**:
```rust
// bindings/c/src/message.rs
// flare_im_list_messages: 调用 api.list(&conversation_id, before_seq, limit).await
// 序列化消息列表为 JSON 数组
```

### 4.4 实现消息操作函数

**描述**: 实现消息操作函数（撤回、编辑、删除、反应、已读）

**输入**:
- `MessageApi` 的 `recall`、`edit`、`delete`、`add_reaction`、`mark_read` 等方法

**输出**:
- `bindings/c/src/message.rs` 中的操作函数

**验收条件**:
- [ ] `flare_im_recall_message` 撤回消息
- [ ] `flare_im_edit_message` 编辑消息
- [ ] `flare_im_delete_message` 删除消息
- [ ] `flare_im_add_reaction` / `flare_im_remove_reaction` 表情反应
- [ ] `flare_im_mark_read` 标记已读

**代码生成提示**:
```rust
// bindings/c/src/message.rs
// 调用对应的 MessageApi 方法
// 通过回调返回结果
```

---

## 任务 5: 会话 API 实现

### 5.1 实现会话查询函数

**描述**: 实现会话查询函数（列表、获取单个）

**输入**:
- `ConversationApi::list`、`ConversationApi::get` 方法

**输出**:
- `bindings/c/src/conversation.rs` 中的查询函数

**验收条件**:
- [ ] `flare_im_get_conversations` 获取会话列表
- [ ] `flare_im_get_conversation` 获取单个会话
- [ ] 支持分页参数（limit、before_id）
- [ ] 返回会话 JSON 或 JSON 数组

**代码生成提示**:
```rust
// bindings/c/src/conversation.rs
// 获取 client.conversation()
// 调用 api.list(limit, before_id.as_deref()).await
// 序列化为 JSON
```

### 5.2 实现会话操作函数

**描述**: 实现会话操作函数（标记已读、置顶、删除、草稿）

**输入**:
- `ConversationApi` 的 `mark_read`、`set_pinned`、`delete`、`update_draft` 方法

**输出**:
- `bindings/c/src/conversation.rs` 中的操作函数

**验收条件**:
- [ ] `flare_im_mark_conversation_read` 标记会话已读
- [ ] `flare_im_set_conversation_pinned` 设置置顶
- [ ] `flare_im_delete_conversation` 删除会话
- [ ] `flare_im_update_conversation_draft` 更新草稿

**代码生成提示**:
```rust
// bindings/c/src/conversation.rs
// 调用对应的 ConversationApi 方法
// 通过回调返回结果
```

---

## 任务 6: 媒体 API 实现

### 6.1 实现媒体上传下载函数

**描述**: 实现媒体上传下载函数

**输入**:
- `MediaApi` 的上传下载方法

**输出**:
- `bindings/c/src/media.rs` 中的媒体函数

**验收条件**:
- [ ] `flare_im_download_file` 下载文件
- [ ] `flare_im_get_media_url` 获取媒体 URL
- [ ] 支持进度回调
- [ ] 正确处理大文件

**代码生成提示**:
```rust
// bindings/c/src/media.rs
// 获取 client.media()
// 调用对应的 MediaApi 方法
// 实现进度回调转发
```

### 6.2 实现缓存管理函数

**描述**: 实现媒体缓存管理函数

**输入**:
- `MediaApi` 的缓存管理方法

**输出**:
- `bindings/c/src/media.rs` 中的缓存函数

**验收条件**:
- [ ] `flare_im_set_media_cache_max_bytes` 设置缓存大小
- [ ] `flare_im_clear_media_cache` 清理缓存
- [ ] `flare_im_media_cache_stats` 获取缓存统计

**代码生成提示**:
```rust
// bindings/c/src/media.rs
// 调用对应的缓存管理方法
```

---

## 任务 7: FFI 入口与 Panic 处理

### 7.1 实现 FFI 入口模块

**描述**: 整合所有模块，导出 FFI 函数

**输入**:
- 各模块的 FFI 函数

**输出**:
- `bindings/c/src/lib.rs` 入口文件

**验收条件**:
- [ ] 导出所有子模块
- [ ] 定义 `catch_panic` 包装函数
- [ ] 初始化 Tokio runtime（如果需要）
- [ ] 设置正确的 crate 类型

**代码生成提示**:
```rust
// bindings/c/src/lib.rs
// 导出 mod error, handle, callback, lifecycle, message, conversation, media, event, string
// 定义 catch_panic<F, R>(f: F) -> R
// 使用 std::panic::catch_unwind 捕获异常
```

### 7.2 更新 Cargo.toml 和构建配置

**描述**: 更新项目配置，添加依赖和构建脚本

**输入**:
- 依赖列表和 cbindgen 配置

**输出**:
- `bindings/c/Cargo.toml`
- `bindings/c/cbindgen.toml`
- `bindings/c/build.rs`

**验收条件**:
- [ ] 添加所有必要依赖
- [ ] 配置 `cdylib` 和 `staticlib` 输出
- [ ] cbindgen 配置正确
- [ ] 构建时自动生成头文件

**代码生成提示**:
```toml
# bindings/c/Cargo.toml
# 添加 flare-im-core-sdk, tokio, serde_json, lazy_static, tracing 依赖
# 配置 [lib] crate-type = ["cdylib", "staticlib"]
# 添加 [build-dependencies] cbindgen
```

---

## 任务 8: Flutter 适配层实现

### 8.1 创建 Flutter 包结构

**描述**: 创建 Flutter Dart 包的基础结构

**输入**:
- Flutter 包规范

**输出**:
- `bindings/flutter/pubspec.yaml`
- `bindings/flutter/lib/` 目录结构

**验收条件**:
- [ ] pubspec.yaml 配置正确
- [ ] 支持多平台（iOS、Android、macOS、Windows、Linux）
- [ ] 配置动态库加载路径

**代码生成提示**:
```yaml
# bindings/flutter/pubspec.yaml
# name: flare_im_sdk
# 配置 platform 支持
# 配置 ffi 依赖
```

### 8.2 实现 FFI 绑定生成

**描述**: 实现 Dart FFI 绑定代码

**输入**:
- C ABI 头文件

**输出**:
- `bindings/flutter/lib/src/bindings.dart`

**验收条件**:
- [ ] 加载各平台动态库
- [ ] 绑定所有 FFI 函数
- [ ] 定义回调类型

**代码生成提示**:
```dart
// bindings/flutter/lib/src/bindings.dart
// import 'dart:ffi';
// 使用 DynamicLibrary.open 加载动态库
// 使用 lookupFunction 绑定函数
```

### 8.3 实现 Dart 封装类

**描述**: 实现 FlareImSdk 主类和类型定义

**输入**:
- FFI 绑定

**输出**:
- `bindings/flutter/lib/flare_im_sdk.dart`
- `bindings/flutter/lib/src/models.dart`

**验收条件**:
- [ ] `FlareImSdk` 类封装所有 API
- [ ] 实现 `events` Stream
- [ ] 定义 `IMMessage`、`IMConversation` 等类型
- [ ] 实现 JSON 序列化/反序列化

**代码生成提示**:
```dart
// bindings/flutter/lib/flare_im_sdk.dart
// class FlareImSdk with _handle, _events
// static Future<FlareImSdk> init(SdkConfig config)
// Stream<SdkEvent> get events
// Future<void> login(String userId, String token)
// Future<IMMessage> createText(String conversationId, String text)
// Future<String> send(IMMessage message)
```

---

## 任务 9: Android 适配层实现

### 9.1 创建 Android 库结构

**描述**: 创建 Android Kotlin 库的基础结构

**输入**:
- Android 库规范

**输出**:
- `bindings/android/build.gradle.kts`
- `bindings/android/src/main/kotlin/` 目录结构

**验收条件**:
- [ ] Gradle 配置正确
- [ ] 配置 JNI 库路径
- [ ] 支持 arm64-v8a、armeabi-v7a、x86_64 架构

**代码生成提示**:
```kotlin
// bindings/android/build.gradle.kts
// 配置 android { defaultConfig { ndk { abiFilters } } }
// 配置 sourceSets { main { jniLibs } }
```

### 9.2 实现 JNI 绑定

**描述**: 实现 Kotlin JNI 绑定代码

**输入**:
- C ABI 头文件

**输出**:
- `bindings/android/src/main/kotlin/com/flare/im/Native.kt`

**验收条件**:
- [ ] 声明所有 native 方法
- [ ] 定义回调接口
- [ ] 加载动态库

**代码生成提示**:
```kotlin
// bindings/android/src/main/kotlin/com/flare/im/Native.kt
// companion object { init { System.loadLibrary("flare_im_core_sdk_ffi") } }
// private external fun nativeNew(): Long
// private external fun nativeInit(handle: Long, configJson: String, callback: ResultCallback)
```

### 9.3 实现 Kotlin 封装类

**描述**: 实现 FlareImSdk 主类和类型定义

**输入**:
- JNI 绑定

**输出**:
- `bindings/android/src/main/kotlin/com/flare/im/FlareImSdk.kt`
- `bindings/android/src/main/kotlin/com/flare/im/Models.kt`

**验收条件**:
- [ ] `FlareImSdk` 类封装所有 API
- [ ] 实现 `events` Flow
- [ ] 使用协程封装异步操作
- [ ] 定义数据类型

**代码生成提示**:
```kotlin
// bindings/android/src/main/kotlin/com/flare/im/FlareImSdk.kt
// class FlareImSdk private constructor(private val handle: Long)
// suspend fun init(config: SdkConfig): FlareImSdk
// val events: Flow<SdkEvent>
// suspend fun login(userId: String, token: String)
```

---

## 任务 10: iOS 适配层实现

### 10.1 创建 Swift 包结构

**描述**: 创建 iOS Swift 包的基础结构

**输入**:
- Swift Package 规范

**输出**:
- `bindings/ios/Package.swift`
- `bindings/ios/Sources/` 目录结构

**验收条件**:
- [ ] Package.swift 配置正确
- [ ] 配置 XCFramework 依赖
- [ ] 支持 iOS 和 macOS 平台

**代码生成提示**:
```swift
// bindings/ios/Package.swift
// let package = Package(name: "FlareImSdk", platforms: [.iOS(.v12), .macOS(.v10_14)])
```

### 10.2 实现 Swift 封装类

**描述**: 实现 FlareImSdk 主类和类型定义

**输入**:
- C ABI 头文件

**输出**:
- `bindings/ios/Sources/FlareImSdk/FlareImSdk.swift`
- `bindings/ios/Sources/FlareImSdk/Models.swift`

**验收条件**:
- [ ] `FlareImSdk` 类封装所有 API
- [ ] 实现 `events` Publisher
- [ ] 使用 async/await 封装异步操作
- [ ] 定义数据类型

**代码生成提示**:
```swift
// bindings/ios/Sources/FlareImSdk/FlareImSdk.swift
// public class FlareImSdk
// public static func initialize(config: SdkConfig) async throws -> FlareImSdk
// public var events: AnyPublisher<SdkEvent, Never>
// public func login(userId: String, token: String) async throws
```

---

## 任务 11: 构建与测试

### 11.1 实现跨平台构建脚本

**描述**: 实现各平台的构建脚本

**输入**:
- 平台构建规范

**输出**:
- 构建 Makefile 或脚本
- CI/CD 配置

**验收条件**:
- [ ] 支持 Linux、macOS、Windows 构建
- [ ] 自动复制动态库到平台目录
- [ ] 生成头文件到正确位置

**代码生成提示**:
```bash
# Makefile 或 build.sh
# cargo build -p flare-im-core-sdk-ffi --release
# 复制动态库到 bindings/flutter/、bindings/android/、bindings/ios/
```

### 11.2 编写单元测试

**描述**: 编写 C ABI 层的单元测试

**输入**:
- FFI 函数实现

**输出**:
- `bindings/c/src/*.rs` 中的 `#[cfg(test)]` 模块

**验收条件**:
- [ ] 测试句柄生命周期
- [ ] 测试错误码映射
- [ ] 测试事件订阅
- [ ] 测试覆盖率 > 80%

**代码生成提示**:
```rust
// bindings/c/src/lifecycle.rs
// #[cfg(test)] mod tests { ... }
// #[test] fn test_handle_lifecycle() { ... }
// #[tokio::test] async fn test_init_login_logout() { ... }
```

### 11.3 编写集成测试

**描述**: 编写端到端集成测试

**输入**:
- 完整 FFI 实现

**输出**:
- `bindings/c/tests/` 目录

**验收条件**:
- [ ] 测试完整生命周期流程
- [ ] 测试消息收发流程
- [ ] 测试事件推送
- [ ] 使用 Mock 服务端

**代码生成提示**:
```rust
// bindings/c/tests/integration_test.rs
// #[tokio::test] async fn test_full_lifecycle() { ... }
```

---

## 任务 12: 文档与示例

### 12.1 更新 README 文档

**描述**: 更新绑定层 README 文档

**输入**:
- 实现完成的绑定层

**输出**:
- `bindings/README.md`
- `bindings/c/README.md`
- `bindings/flutter/README.md`

**验收条件**:
- [ ] 说明架构设计
- [ ] 说明各平台使用方法
- [ ] 包含 API 参考

**代码生成提示**:
```markdown
# bindings/README.md
# 说明统一 C ABI 架构
# 各平台适配层使用指南
# API 参考
```

### 12.2 更新 Flutter 示例

**描述**: 更新 examples/flare 示例应用

**输入**:
- Flutter 适配层

**输出**:
- `examples/flare/lib/` 更新

**验收条件**:
- [ ] 使用新的 C ABI 绑定
- [ ] 演示登录、消息收发、会话列表
- [ ] 演示事件监听

**代码生成提示**:
```dart
// examples/flare/lib/main.dart
// import 'package:flare_im_sdk/flare_im_sdk.dart';
// 演示完整 IM 功能
```

### 12.3 添加 API 文档注释

**描述**: 为所有公开 API 添加文档注释

**输入**:
- FFI 函数实现

**输出**:
- 带文档注释的代码

**验收条件**:
- [ ] 所有 `#[no_mangle]` 函数有文档注释
- [ ] 文档包含参数说明、返回值说明、使用示例
- [ ] 生成 Rustdoc 文档

**代码生成提示**:
```rust
// 为每个 FFI 函数添加 /// 文档注释
// 说明参数、返回值、使用方法
```

---

## 任务依赖关系

```
任务 1 (基础设施) ─┬─> 任务 2 (生命周期)
                    ├─> 任务 3 (事件订阅)
                    └─> 任务 7 (FFI 入口)

任务 2 (生命周期) ───> 任务 4 (消息 API)
                    └──> 任务 5 (会话 API)
                    └──> 任务 6 (媒体 API)

任务 3 (事件订阅) ───> 任务 8 (Flutter 适配)
                    └──> 任务 9 (Android 适配)
                    └──> 任务 10 (iOS 适配)

任务 4-6 (API 实现) ─> 任务 7 (FFI 入口)

任务 7 (FFI 入口) ───> 任务 8-10 (平台适配)
                    └──> 任务 11 (构建测试)

任务 8-10 (平台适配) ─> 任务 12 (文档示例)
```

---

## 执行优先级

| 优先级 | 任务 | 原因 |
|--------|------|------|
| P0 | 任务 1、2、7 | 核心基础设施，其他任务依赖 |
| P1 | 任务 3、4、5、6 | 核心 API 实现 |
| P2 | 任务 8、9、10 | 平台适配层 |
| P3 | 任务 11、12 | 测试和文档 |

---

## 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| 异步回调线程安全 | Crash | 使用 `Send`/`Sync` 约束，平台侧处理线程切换 |
| 内存泄漏 | 性能下降 | 严格的内存所有权文档，测试覆盖 |
| cbindgen 兼容性 | 构建失败 | 测试多平台构建，备用手动头文件 |
| 平台差异 | 行为不一致 | 统一测试用例，CI 覆盖所有平台 |
