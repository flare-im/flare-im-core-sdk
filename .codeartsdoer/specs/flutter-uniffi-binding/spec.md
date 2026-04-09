# 统一 C ABI 绑定层需求规格

## 1. 组件定位

### 1.1 核心职责

本组件负责为所有平台（iOS、Android、Flutter、鸿蒙、Desktop）提供**统一的 C ABI 绑定层**，作为 Rust SDK 与各平台之间的唯一桥梁。遵循"**Rust 核心单一真相 + 统一 C ABI + 极薄平台适配层**"架构原则。

### 1.2 架构原则

1. **Rust 只维护一套核心业务** - 所有业务逻辑在 `flare-im-core-sdk` 中实现
2. **对外统一导出 C ABI** - `bindings/c` 是唯一的 FFI 层，所有平台共用
3. **平台极薄适配层** - 各平台适配层代码量控制在 200-400 LOC
4. **接口统一设计** - 采用"句柄 + 错误码 + JSON/bytes + 回调"统一模式

### 1.3 核心输入

1. **Rust SDK API**: `flare-im-core-sdk` 提供的公共接口（IMClient、MessageApi、ConversationApi、MediaApi）
2. **平台调用请求**: 通过 C ABI 传入的句柄、参数 JSON 和回调函数
3. **事件回调注册**: 平台注册的事件回调，用于接收 SDK 推送的异步事件

### 1.4 核心输出

1. **C ABI 动态库**: `libflare_im_core_sdk_ffi.so/dylib/dll`
2. **C 头文件**: `flare_im_core_sdk_ffi.h`（由 cbindgen 自动生成）
3. **平台适配包**: Flutter Dart 包、Android Kotlin 包、iOS Swift 包、鸿蒙 NAPI 包
4. **事件推送**: 通过回调向各平台推送 SDK 事件

### 1.5 职责边界

本组件**不负责**:

1. 具体业务逻辑实现（由 `flare-im-core-sdk` 提供）
2. 平台 UI 组件开发（由上层应用负责）
3. 平台特定的持久化存储实现（由 core-sdk 的 Store trait 实现）
4. 网络层实现（由 core-sdk 的 Transport 层负责）

---

## 2. 领域术语

**句柄 (Handle)**
: 跨语言传递 Rust 对象的不透明标识，本质是指针或 ID。平台侧只持有句柄，不直接访问对象内存。
: 备注: 句柄生命周期由 Rust 侧管理，平台侧通过 `flare_im_*_free` 释放。

**C ABI (C Application Binary Interface)**
: C 语言标准二进制接口，所有平台 FFI 都支持。通过 `extern "C"` 声明函数，`cbindgen` 生成头文件。
: 备注: 相比 UniFFI，C ABI 更通用、更稳定、无运行时依赖。

**回调模式 (Callback Pattern)**
: 异步操作的标准 FFI 模式。调用时传入回调函数指针和上下文指针，操作完成后回调通知结果。
: 备注: 回调在 Rust 侧的 Tokio runtime 中执行，需确保线程安全。

**JSON 消息 (JSON Message)**
: 跨语言传递复杂数据的序列化格式。所有结构化参数和返回值都使用 JSON 字符串传递。
: 备注: JSON 由 `serde_json` 序列化，平台侧用各语言 JSON 库反序列化。

**事件订阅 (Event Subscription)**
: 平台注册全局事件回调，接收 SDK 推送的所有事件。返回订阅句柄用于取消订阅。
: 备注: 事件通过 `FlareEventCallback` 推送，包含 `event_type` 和 `event_json`。

---

## 3. 架构设计

### 3.1 分层架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         Platform Thin Adapters                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │   Flutter   │  │   Android   │  │     iOS     │  │   鸿蒙      │       │
│  │  (dart:ffi) │  │   (JNI)     │  │  (Swift FFI)│  │ (NAPI/C++)  │       │
│  │  ~200 LOC   │  │  ~300 LOC   │  │  ~200 LOC   │  │  ~300 LOC   │       │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘       │
│         │                │                │                │               │
│         │  统一 API      │  统一 API      │  统一 API      │  统一 API     │
│         ▼                ▼                ▼                ▼               │
└─────────┼────────────────┼────────────────┼────────────────┼───────────────┘
          │                │                │                │
          └────────────────┴────────────────┴────────────────┘
                                   │
                          C ABI (统一接口)
                                   │
┌──────────────────────────────────┼──────────────────────────────────────────┐
│  ┌───────────────────────────────▼──────────────────────────────────────┐   │
│  │                    bindings/c (唯一 FFI 层)                           │   │
│  │                                                                       │   │
│  │  • 句柄管理: flare_im_new / flare_im_free                            │   │
│  │  • 生命周期: init / login / logout                                   │   │
│  │  • 消息 API: create_text / send_message / get_messages               │   │
│  │  • 会话 API: get_conversations / mark_read                           │   │
│  │  • 媒体 API: send_media / download_file                              │   │
│  │  • 事件订阅: subscribe_events / unsubscribe                          │   │
│  │                                                                       │   │
│  │  设计原则：句柄 + 错误码 + JSON/bytes + 回调                           │   │
│  └───────────────────────────────┬──────────────────────────────────────┘   │
│                    flare-im-core-sdk-ffi                                   │
└──────────────────────────────────┼──────────────────────────────────────────┘
                                   │
┌──────────────────────────────────▼──────────────────────────────────────────┐
│                         flare-im-core-sdk                                   │
│  (Rust 核心业务，单一真相来源)                                               │
│  • IMClient, MessageApi, ConversationApi, MediaApi                         │
│  • EventBus, SdkEvent, Domain Models                                        │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 目录结构

```
bindings/
├── c/                          # 统一 C ABI 层（唯一 FFI 实现）
│   ├── Cargo.toml
│   ├── cbindgen.toml           # 头文件生成配置
│   ├── build.rs                # cbindgen 构建脚本
│   └── src/
│       ├── lib.rs              # FFI 入口
│       ├── handle.rs           # 句柄管理
│       ├── lifecycle.rs        # 生命周期 API
│       ├── message.rs          # 消息 API
│       ├── conversation.rs     # 会话 API
│       ├── media.rs            # 媒体 API
│       ├── event.rs            # 事件订阅
│       ├── callback.rs         # 回调管理
│       └── error.rs            # 错误码定义
│
├── flutter/                    # Flutter 极薄适配层
│   ├── pubspec.yaml
│   └── lib/
│       ├── flare_im_sdk.dart   # 主入口 (~100 LOC)
│       ├── bindings.dart       # FFI 绑定生成 (~50 LOC)
│       └── models.dart         # Dart 类型定义 (~50 LOC)
│
├── android/                    # Android 极薄适配层
│   ├── build.gradle.kts
│   └── src/main/kotlin/
│       ├── FlareImSdk.kt       # 主入口 (~150 LOC)
│       └── Models.kt           # Kotlin 类型定义 (~100 LOC)
│
├── ios/                        # iOS 极薄适配层
│   ├── Package.swift
│   └── Sources/
│       ├── FlareImSdk.swift    # 主入口 (~100 LOC)
│       └── Models.swift        # Swift 类型定义 (~50 LOC)
│
├── harmony/                    # 鸿蒙极薄适配层
│   ├── oh-package.json5
│   └── src/main/cpp/
│       ├── flare_im_sdk.cpp    # NAPI 绑定 (~200 LOC)
│       └── Models.ts           # TypeScript 类型定义 (~100 LOC)
│
└── tauri/                      # Tauri 绑定（已有实现，保持不变）
```

### 3.3 角色与边界

#### 3.3.1 核心角色

- **Rust SDK 维护者**: 维护 `flare-im-core-sdk` 和 `bindings/c`，确保 C ABI 稳定
- **平台适配开发者**: 开发各平台极薄适配层，封装 C ABI 为平台惯用 API
- **应用开发者**: 使用平台适配包开发 IM 应用

#### 3.3.2 外部系统

- **flare-im-core-sdk**: Rust 核心库，提供 IM 客户端全部能力
- **cbindgen**: C 头文件自动生成工具
- **平台 FFI 机制**: dart:ffi、JNI、Swift FFI、NAPI
- **平台构建系统**: Gradle、Xcode、CMake、OHOS Build

---

## 4. DFX 约束

### 4.1 性能

1. **FFI 调用延迟**: 单次 FFI 调用开销 < 1μs
   - 验收条件: [FFI 调用基准测试] → [P99 延迟 < 1μs]

2. **JSON 序列化开销**: 参数序列化 + 结果反序列化 < 100μs（1KB 数据）
   - 验收条件: [JSON 序列化测试] → [1KB 数据处理 < 100μs]

3. **事件推送延迟**: 从 Rust 事件发布到平台回调执行 < 10ms
   - 验收条件: [SDK 发布事件] → [平台回调执行延迟 < 10ms]

4. **内存开销**: C ABI 层额外内存开销 < 5MB
   - 验收条件: [内存分析] → [C ABI 层内存占用 < 5MB]

5. **动态库大小**: Release 编译后动态库 < 5MB（strip 后）
   - 验收条件: [编译产物检查] → [动态库大小 < 5MB]

### 4.2 可靠性

1. **内存安全**: 所有跨语言传递的对象必须正确管理生命周期，无内存泄漏
   - 验收条件: [Valgrind/ASan 测试] → [无内存泄漏]

2. **线程安全**: 回调可在任意线程执行，平台侧需处理线程切换
   - 验收条件: [多线程压力测试] → [无数据竞争]

3. **错误传递**: Rust 错误必须完整映射到 C 错误码和消息
   - 验收条件: [Rust 返回错误] → [C 回调收到对应错误码和消息]

4. **句柄有效性**: 使用无效句柄必须返回错误，不可 crash
   - 验收条件: [传入无效句柄] → [返回 FLARE_ERR_INVALID_HANDLE]

5. **优雅关闭**: 所有资源正确释放，无资源泄漏
   - 验收条件: [调用 flare_im_free] → [所有资源释放]

### 4.3 兼容性

1. **ABI 稳定性**: C ABI 签名变更必须遵循 SemVer
   - 验收条件: [版本升级] → [旧版平台适配层仍可编译]

2. **平台支持**: 必须支持 iOS 12+、Android API 21+、macOS 10.14+、Windows 10+、鸿蒙 4.0+
   - 验收条件: [各平台最低版本测试] → [正常运行]

3. **编译器兼容**: 支持 GCC、Clang、MSVC 三大编译器
   - 验收条件: [各编译器编译] → [生成正确动态库]

4. **字节序**: 支持大端和小端架构（虽然主流平台都是小端）
   - 验收条件: [字节序测试] → [正确处理]

### 4.4 可维护性

1. **头文件自动生成**: C 头文件必须由 cbindgen 自动生成，禁止手动编写
   - 验收条件: [代码审查] → [无手动编写的头文件]

2. **API 文档**: 所有 C ABI 函数必须有文档注释
   - 验收条件: [文档检查] → [所有公开 API 有注释]

3. **测试覆盖**: C ABI 层单元测试覆盖率 > 80%
   - 验收条件: [测试报告] → [覆盖率 > 80%]

---

## 5. 核心能力

### 5.1 C ABI 接口设计

#### 5.1.1 基础类型定义

```c
// ===== 句柄类型 =====
typedef struct FlareImHandle { uint64_t id; } FlareImHandle;
typedef struct FlareEventSubscription { uint64_t id; } FlareEventSubscription;

// ===== 错误码 =====
typedef enum FlareErrorCode {
    FLARE_OK = 0,
    
    // 连接错误 (1xxx)
    FLARE_ERR_NOT_CONNECTED = 1001,
    FLARE_ERR_CONNECTION_FAILED = 1002,
    FLARE_ERR_CONNECTION_TIMEOUT = 1003,
    
    // 参数错误 (2xxx)
    FLARE_ERR_INVALID_PARAM = 2001,
    FLARE_ERR_INVALID_HANDLE = 2002,
    FLARE_ERR_INVALID_JSON = 2003,
    
    // 网络错误 (3xxx)
    FLARE_ERR_NETWORK = 3001,
    FLARE_ERR_TIMEOUT = 3002,
    
    // 认证错误 (4xxx)
    FLARE_ERR_UNAUTHORIZED = 4001,
    FLARE_ERR_TOKEN_EXPIRED = 4002,
    FLARE_ERR_KICKED_OFF = 4003,
    
    // 存储错误 (5xxx)
    FLARE_ERR_STORAGE = 5001,
    FLARE_ERR_NOT_FOUND = 5002,
    
    // 内部错误 (9xxx)
    FLARE_ERR_INTERNAL = 9001,
    FLARE_ERR_UNKNOWN = 9999,
} FlareErrorCode;
```

#### 5.1.2 回调类型定义

```c
// ===== 基础回调 =====
/// 结果回调（无返回值）
/// @param context 用户上下文指针
/// @param code 错误码，FLARE_OK 表示成功
/// @param message 错误消息，成功时为 NULL
typedef void (*FlareResultCallback)(void* context, FlareErrorCode code, const char* message);

/// 字符串回调（返回单个字符串）
/// @param result 结果字符串，需调用 flare_im_string_free 释放
typedef void (*FlareStringCallback)(void* context, FlareErrorCode code, const char* result);

/// JSON 回调（返回 JSON 字符串）
/// @param json JSON 结果字符串，需调用 flare_im_string_free 释放
typedef void (*FlareJsonCallback)(void* context, FlareErrorCode code, const char* json);

/// 字节数组回调（返回二进制数据）
/// @param data 数据指针，需调用 flare_im_bytes_free 释放
/// @param len 数据长度
typedef void (*FlareBytesCallback)(void* context, FlareErrorCode code, const uint8_t* data, size_t len);

// ===== 事件回调 =====
/// 事件回调（推送 SDK 事件）
/// @param event_type 事件类型，如 "message.received"、"connection.connected"
/// @param event_json 事件 JSON 数据，需调用 flare_im_string_free 释放
typedef void (*FlareEventCallback)(void* context, const char* event_type, const char* event_json);

// ===== 进度回调 =====
/// 上传进度回调
/// @param uploaded_bytes 已上传字节数
/// @param total_bytes 总字节数
typedef void (*FlareUploadProgressCallback)(void* context, uint64_t uploaded_bytes, uint64_t total_bytes);

/// 下载进度回调
typedef void (*FlareDownloadProgressCallback)(void* context, uint64_t downloaded_bytes, uint64_t total_bytes);
```

#### 5.1.3 生命周期 API

```c
// ===== 创建与销毁 =====

/// 创建 SDK 实例
/// @return SDK 句柄
FlareImHandle flare_im_new(void);

/// 释放 SDK 实例
/// @param handle SDK 句柄
/// @note 释放后会断开连接并清理所有资源
void flare_im_free(FlareImHandle handle);

// ===== 初始化与登录 =====

/// 初始化 SDK
/// @param handle SDK 句柄
/// @param config_json 配置 JSON，格式见 SdkConfig
/// @param context 用户上下文，将传递给 callback
/// @param callback 结果回调
void flare_im_init(FlareImHandle handle, const char* config_json, 
                   void* context, FlareResultCallback callback);

/// 登录
/// @param handle SDK 句柄
/// @param user_id 用户 ID
/// @param token JWT Token
/// @param context 用户上下文
/// @param callback 结果回调
void flare_im_login(FlareImHandle handle, const char* user_id, const char* token,
                    void* context, FlareResultCallback callback);

/// 登出
void flare_im_logout(FlareImHandle handle, void* context, FlareResultCallback callback);

// ===== 状态查询（同步） =====

/// 是否已连接
bool flare_im_is_connected(FlareImHandle handle);

/// 当前用户 ID
/// @return 用户 ID 字符串，需调用 flare_im_string_free 释放；未登录返回 NULL
const char* flare_im_current_user_id(FlareImHandle handle);

/// SDK 状态
/// @return 状态字符串，如 "Connected"、"Disconnected"、"Reconnecting"
const char* flare_im_state(FlareImHandle handle);
```

#### 5.1.4 事件订阅 API

```c
/// 订阅 SDK 事件
/// @param handle SDK 句柄
/// @param context 用户上下文，将传递给 callback
/// @param callback 事件回调
/// @return 订阅句柄，用于取消订阅
FlareEventSubscription flare_im_subscribe_events(FlareImHandle handle, 
                                                  void* context, 
                                                  FlareEventCallback callback);

/// 取消事件订阅
void flare_im_unsubscribe(FlareEventSubscription subscription);
```

#### 5.1.5 消息 API

```c
// ===== 消息构建 =====

/// 创建文本消息
/// @param conversation_id 会话 ID
/// @param text 文本内容
/// @return 消息 JSON，需调用 flare_im_string_free 释放
void flare_im_create_text_message(FlareImHandle handle, 
                                   const char* conversation_id, 
                                   const char* text,
                                   void* context, 
                                   FlareJsonCallback callback);

/// 创建图片消息
/// @param conversation_id 会话 ID
/// @param local_path 本地图片路径
/// @param thumbnail_path 缩略图路径，可为 NULL
void flare_im_create_image_message(FlareImHandle handle,
                                    const char* conversation_id,
                                    const char* local_path,
                                    const char* thumbnail_path,
                                    void* context,
                                    FlareJsonCallback callback);

/// 创建文件消息
void flare_im_create_file_message(FlareImHandle handle,
                                   const char* conversation_id,
                                   const char* local_path,
                                   const char* file_name,
                                   void* context,
                                   FlareJsonCallback callback);

// ===== 消息发送 =====

/// 发送消息
/// @param message_json 消息 JSON，由 create_*_message 返回
/// @return client_msg_id，需调用 flare_im_string_free 释放
void flare_im_send_message(FlareImHandle handle, 
                            const char* message_json,
                            void* context, 
                            FlareStringCallback callback);

/// 发送媒体消息（带进度）
void flare_im_send_media_message(FlareImHandle handle,
                                  const char* message_json,
                                  void* progress_context,
                                  FlareUploadProgressCallback progress_callback,
                                  void* result_context,
                                  FlareStringCallback result_callback);

// ===== 消息查询 =====

/// 获取消息
/// @param message_id 消息 ID（server_msg_id 或 client_msg_id）
/// @return 消息 JSON，不存在时返回 NULL
void flare_im_get_message(FlareImHandle handle,
                           const char* conversation_id,
                           const char* message_id,
                           void* context,
                           FlareJsonCallback callback);

/// 获取消息列表
/// @param before_seq 起始序列号，0 表示从最新开始
/// @param limit 数量限制
/// @return 消息 JSON 数组
void flare_im_list_messages(FlareImHandle handle,
                             const char* conversation_id,
                             uint64_t before_seq,
                             int32_t limit,
                             void* context,
                             FlareJsonCallback callback);

/// 搜索消息
/// @param query_json 搜索条件 JSON
/// @return 匹配的消息 JSON 数组
void flare_im_search_messages(FlareImHandle handle,
                               const char* query_json,
                               void* context,
                               FlareJsonCallback callback);

// ===== 消息操作 =====

/// 撤回消息
void flare_im_recall_message(FlareImHandle handle,
                              const char* conversation_id,
                              const char* message_id,
                              void* context,
                              FlareResultCallback callback);

/// 编辑消息
void flare_im_edit_message(FlareImHandle handle,
                            const char* conversation_id,
                            const char* message_id,
                            const char* new_content_json,
                            void* context,
                            FlareResultCallback callback);

/// 删除消息
void flare_im_delete_message(FlareImHandle handle,
                              const char* conversation_id,
                              const char* message_id,
                              void* context,
                              FlareResultCallback callback);

/// 添加表情反应
void flare_im_add_reaction(FlareImHandle handle,
                            const char* conversation_id,
                            const char* message_id,
                            const char* emoji,
                            void* context,
                            FlareResultCallback callback);

/// 移除表情反应
void flare_im_remove_reaction(FlareImHandle handle,
                               const char* conversation_id,
                               const char* message_id,
                               const char* emoji,
                               void* context,
                               FlareResultCallback callback);

/// 标记已读
void flare_im_mark_read(FlareImHandle handle,
                         const char* conversation_id,
                         uint64_t read_seq,
                         void* context,
                         FlareResultCallback callback);
```

#### 5.1.6 会话 API

```c
/// 获取会话列表
/// @param limit 数量限制，0 表示使用默认值
/// @param before_id 起始会话 ID，NULL 表示从最新开始
/// @return 会话 JSON 数组
void flare_im_get_conversations(FlareImHandle handle,
                                 int32_t limit,
                                 const char* before_id,
                                 void* context,
                                 FlareJsonCallback callback);

/// 获取单个会话
void flare_im_get_conversation(FlareImHandle handle,
                                const char* conversation_id,
                                void* context,
                                FlareJsonCallback callback);

/// 标记会话已读
void flare_im_mark_conversation_read(FlareImHandle handle,
                                      const char* conversation_id,
                                      void* context,
                                      FlareResultCallback callback);

/// 设置会话置顶
void flare_im_set_conversation_pinned(FlareImHandle handle,
                                       const char* conversation_id,
                                       bool pinned,
                                       void* context,
                                       FlareResultCallback callback);

/// 删除会话
void flare_im_delete_conversation(FlareImHandle handle,
                                   const char* conversation_id,
                                   void* context,
                                   FlareResultCallback callback);

/// 更新会话草稿
void flare_im_update_conversation_draft(FlareImHandle handle,
                                         const char* conversation_id,
                                         const char* draft,
                                         void* context,
                                         FlareResultCallback callback);
```

#### 5.1.7 媒体 API

```c
/// 下载文件
void flare_im_download_file(FlareImHandle handle,
                             const char* url,
                             const char* local_path,
                             void* progress_context,
                             FlareDownloadProgressCallback progress_callback,
                             void* result_context,
                             FlareResultCallback result_callback);

/// 获取媒体 URL
void flare_im_get_media_url(FlareImHandle handle,
                             const char* media_id,
                             void* context,
                             FlareStringCallback callback);

/// 设置媒体缓存大小
void flare_im_set_media_cache_max_bytes(FlareImHandle handle,
                                         uint64_t max_bytes,
                                         void* context,
                                         FlareResultCallback callback);

/// 清理媒体缓存
void flare_im_clear_media_cache(FlareImHandle handle,
                                 void* context,
                                 FlareResultCallback callback);
```

#### 5.1.8 内存管理

```c
/// 释放字符串内存
void flare_im_string_free(const char* ptr);

/// 释放字节数组内存
void flare_im_bytes_free(const uint8_t* ptr);
```

### 5.2 JSON 数据格式

#### 5.2.1 SdkConfig

```json
{
  "environment": "prod",
  "data_url": "file:///path/to/data",
  "ws_url": "wss://im.example.com",
  "http_url": "https://api.example.com",
  "log_level": "info"
}
```

#### 5.2.2 IMMessage

```json
{
  "client_msg_id": "uuid-v4",
  "server_msg_id": "server-assigned-id",
  "conversation_id": "conv_123",
  "sender_id": "user_456",
  "content_type": 1,
  "content": { "text": "Hello" },
  "status": 2,
  "seq": 100,
  "created_at": 1704067200000,
  "local_state": {
    "sending": false,
    "failed": false,
    "is_local": false
  }
}
```

#### 5.2.3 IMConversation

```json
{
  "conversation_id": "conv_123",
  "type": 1,
  "name": "Group Name",
  "avatar": "https://...",
  "last_message": { "...": "..." },
  "unread_count": 5,
  "is_pinned": true,
  "draft": "draft text",
  "updated_at": 1704067200000
}
```

#### 5.2.4 SdkEvent

事件通过 `FlareEventCallback` 推送，格式为：

```c
// event_type: "connection.connected"
// event_json: {}

// event_type: "connection.disconnected"
// event_json: { "reason": "network_error" }

// event_type: "message.received"
// event_json: { "message": { "...": "..." } }

// event_type: "message.send_ack"
// event_json: { "client_msg_id": "...", "server_msg_id": "...", "success": true }

// event_type: "conversation.updated"
// event_json: { "conversation_id": "..." }
```

### 5.3 平台适配层设计

#### 5.3.1 Flutter 适配层

```dart
// lib/src/flare_im_sdk.dart
import 'dart:ffi';
import 'dart:convert';

class FlareImSdk {
  final DynamicLibrary _lib;
  final int _handle;
  final StreamController<SdkEvent> _events = StreamController.broadcast();
  
  FlareImSdk._(this._lib, this._handle) {
    _subscribeEvents();
  }
  
  /// 初始化 SDK
  static Future<FlareImSdk> init(SdkConfig config) async {
    final lib = DynamicLibrary.open(_libName);
    final handle = _flareImNew(lib);
    await _callAsync(lib, handle, 'flare_im_init', config.toJson());
    return FlareImSdk._(lib, handle);
  }
  
  /// 事件流
  Stream<SdkEvent> get events => _events.stream;
  
  /// 登录
  Future<void> login(String userId, String token) =>
      _callAsync(_lib, _handle, 'flare_im_login', {'user_id': userId, 'token': token});
  
  /// 创建文本消息
  Future<IMMessage> createText(String convId, String text) =>
      _callJsonAsync(_lib, _handle, 'flare_im_create_text_message', 
                     [convId, text]);
  
  /// 发送消息
  Future<String> send(IMMessage message) =>
      _callStringAsync(_lib, _handle, 'flare_im_send_message', message.toJson());
  
  // ... 其他方法
}
```

#### 5.3.2 Android 适配层

```kotlin
// src/main/kotlin/FlareImSdk.kt
class FlareImSdk private constructor(private val handle: Long) {
    private val eventEmitter = EventEmitter()
    
    companion object {
        init { System.loadLibrary("flare_im_core_sdk_ffi") }
        
        suspend fun init(config: SdkConfig): FlareImSdk {
            val handle = nativeNew()
            nativeInit(handle, config.toJson())
            return FlareImSdk(handle).apply { subscribeEvents() }
        }
    }
    
    val events: Flow<SdkEvent> get() = eventEmitter.flow
    
    suspend fun login(userId: String, token: String) = 
        callAsync("flare_im_login") { nativeLogin(handle, userId, token, it) }
    
    suspend fun createText(convId: String, text: String): IMMessage =
        callJsonAsync("flare_im_create_text_message") { nativeCreateText(handle, convId, text, it) }
    
    // ... 其他方法
    
    private external fun nativeNew(): Long
    private external fun nativeInit(handle: Long, configJson: String, callback: ResultCallback)
    private external fun nativeLogin(handle: Long, userId: String, token: String, callback: ResultCallback)
    // ... 其他 native 方法
}
```

---

## 6. 数据约束

### 6.1 句柄有效性

1. **FlareImHandle.id**: 必须大于 0，0 表示无效句柄
2. **句柄生命周期**: 从 `flare_im_new` 创建，到 `flare_im_free` 释放
3. **线程安全**: 句柄可在任意线程使用，内部保证线程安全

### 6.2 字符串参数

1. **编码**: 所有字符串参数必须是 UTF-8 编码
2. **所有权**: 传入字符串由调用方管理，传出字符串由 SDK 分配、调用方释放
3. **NULL 处理**: 可选参数可传 NULL，必选参数传 NULL 返回 `FLARE_ERR_INVALID_PARAM`

### 6.3 JSON 参数

1. **格式**: 必须是合法的 JSON 字符串
2. **字段**: 必须包含必需字段，可选字段可省略
3. **错误**: JSON 解析失败返回 `FLARE_ERR_INVALID_JSON`

### 6.4 回调约束

1. **执行线程**: 回调在 Tokio runtime 线程中执行，非主线程
2. **生命周期**: 回调在操作完成或取消后不再被调用
3. **异常安全**: 回调中不应抛出异常（C 侧无法捕获）

---

## 7. 集成要求

### 7.1 构建要求

1. **Rust 编译**: 使用 `cargo build -p flare-im-core-sdk-ffi --release` 编译
2. **头文件生成**: 编译时自动生成 `flare_im_core_sdk_ffi.h`
3. **平台集成**: 各平台构建脚本自动复制动态库和头文件

### 7.2 Flutter 集成

1. **pubspec.yaml**: 添加 `flare_im_sdk` 依赖
2. **平台配置**: iOS/Android 配置动态库加载路径
3. **初始化**: 应用启动时调用 `FlareImSdk.init`

### 7.3 示例应用

1. **examples/flare**: 更新现有 Flutter 示例，使用新的 C ABI 绑定
2. **示例功能**: 登录、消息收发、会话列表、事件监听
