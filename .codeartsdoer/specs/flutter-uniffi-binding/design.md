# 统一 C ABI 绑定层技术设计

## 1. 设计概述

### 1.1 设计目标

将现有 `bindings/c` 从 TODO 状态重构为完整的统一 C ABI 层，实现：
- **单一 FFI 层**：所有平台共用同一套 C ABI
- **句柄 + 回调模式**：异步操作通过回调返回，同步操作直接返回
- **JSON 序列化**：复杂数据通过 JSON 传递，避免复杂类型映射
- **自动头文件生成**：使用 cbindgen 自动生成 C 头文件

### 1.2 技术栈

| 组件 | 技术 | 版本 |
|------|------|------|
| 语言 | Rust | Edition 2024 (1.94.0) |
| 异步运行时 | Tokio | 1.x |
| JSON 序列化 | serde_json | 1.x |
| 头文件生成 | cbindgen | 0.26 |
| 核心依赖 | flare-im-core-sdk | workspace |

### 1.3 设计原则

1. **FFI 安全边界**：所有 `extern "C"` 函数必须捕获 panic，不可让异常跨越 FFI
2. **内存所有权清晰**：传入参数由调用方管理，传出参数由 SDK 分配、调用方释放
3. **线程安全**：句柄内部使用 `Arc<RwLock<>>`，回调可在任意线程执行
4. **零拷贝优先**：大块数据（如媒体文件）避免不必要的拷贝

---

## 2. 架构设计

### 2.1 模块架构

```
bindings/c/src/
├── lib.rs              # FFI 入口，导出所有公开函数
├── error.rs            # 错误码定义与映射
├── handle.rs           # 句柄管理（SDK 实例、订阅）
├── callback.rs         # 回调管理（存储、调度）
├── lifecycle.rs        # 生命周期 API（init/login/logout）
├── message.rs          # 消息 API（构建、发送、查询、操作）
├── conversation.rs     # 会话 API（列表、操作）
├── media.rs            # 媒体 API（上传、下载、缓存）
├── event.rs            # 事件订阅 API
├── json.rs             # JSON 辅助工具
└── string.rs           # 字符串内存管理
```

### 2.2 核心数据结构

#### 2.2.1 句柄管理

```rust
// handle.rs
use std::sync::{Arc, RwLock, atomic::{AtomicU64, Ordering}};
use flare_im_core_sdk::client::IMClient;

/// 全局句柄 ID 生成器
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// SDK 实例句柄（C 侧可见）
#[repr(C)]
pub struct FlareImHandle {
    pub id: u64,
}

/// SDK 实例内部状态
pub struct SdkInstance {
    pub client: IMClient,
    pub runtime: tokio::runtime::Handle,
    pub event_subscriptions: RwLock<Vec<Arc<EventSubscriptionInner>>>,
}

/// 全局句柄表（懒加载）
lazy_static::lazy_static! {
    static ref HANDLE_TABLE: RwLock<HashMap<u64, Arc<SdkInstance>>> = RwLock::new(HashMap::new());
}

/// 从句柄获取实例
pub fn get_instance(handle: FlareImHandle) -> Result<Arc<SdkInstance>, FlareErrorCode> {
    let table = HANDLE_TABLE.read().map_err(|_| FlareErrorCode::InternalError)?;
    table.get(&handle.id).cloned().ok_or(FlareErrorCode::InvalidHandle)
}
```

#### 2.2.2 回调管理

```rust
// callback.rs
use std::ffi::c_void;

/// 结果回调
pub type FlareResultCallback = extern "C" fn(*mut c_void, FlareErrorCode, *const i8);

/// JSON 回调
pub type FlareJsonCallback = extern "C" fn(*mut c_void, FlareErrorCode, *const i8);

/// 事件回调
pub type FlareEventCallback = extern "C" fn(*mut c_void, *const i8, *const i8);

/// 回调上下文（包装用户上下文和回调函数）
pub struct CallbackContext<C> {
    pub user_context: *mut c_void,
    pub callback: C,
}

// 安全：回调上下文跨线程传递
unsafe impl<C> Send for CallbackContext<C> {}
unsafe impl<C> Sync for CallbackContext<C> {}

/// 调用结果回调
pub fn invoke_result_callback(
    ctx: CallbackContext<FlareResultCallback>,
    result: Result<(), FlareErrorCode>,
) {
    let (code, msg) = match result {
        Ok(()) => (FlareErrorCode::Ok, std::ptr::null()),
        Err(e) => (e, CString::new(e.to_string()).unwrap().into_raw()),
    };
    (ctx.callback)(ctx.user_context, code, msg);
}
```

#### 2.2.3 错误码映射

```rust
// error.rs
use flare_im_core_sdk::ErrorCode;

/// C ABI 错误码
#[repr(C, i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlareErrorCode {
    Ok = 0,
    
    // 连接错误 (1xxx)
    NotConnected = 1001,
    ConnectionFailed = 1002,
    ConnectionTimeout = 1003,
    
    // 参数错误 (2xxx)
    InvalidParam = 2001,
    InvalidHandle = 2002,
    InvalidJson = 2003,
    
    // 网络错误 (3xxx)
    Network = 3001,
    Timeout = 3002,
    
    // 认证错误 (4xxx)
    Unauthorized = 4001,
    TokenExpired = 4002,
    KickedOff = 4003,
    
    // 存储错误 (5xxx)
    Storage = 5001,
    NotFound = 5002,
    
    // 内部错误 (9xxx)
    InternalError = 9001,
    Unknown = 9999,
}

/// 从 SDK ErrorCode 映射到 C ABI 错误码
impl From<ErrorCode> for FlareErrorCode {
    fn from(code: ErrorCode) -> Self {
        match code {
            ErrorCode::NotConnected => Self::NotConnected,
            ErrorCode::InvalidParameter => Self::InvalidParam,
            ErrorCode::AuthenticationFailed => Self::Unauthorized,
            ErrorCode::TokenExpired => Self::TokenExpired,
            ErrorCode::NetworkError => Self::Network,
            ErrorCode::OperationTimeout => Self::Timeout,
            ErrorCode::InternalError => Self::InternalError,
            _ => Self::Unknown,
        }
    }
}

/// 从 FlareError 映射
impl From<&flare_im_core_sdk::FlareError> for FlareErrorCode {
    fn from(err: &flare_im_core_sdk::FlareError) -> Self {
        err.code().into()
    }
}
```

### 2.3 事件系统设计

#### 2.3.1 事件订阅

```rust
// event.rs
use std::sync::Arc;
use tokio::sync::broadcast;

/// 事件订阅内部状态
pub struct EventSubscriptionInner {
    pub id: u64,
    pub callback: FlareEventCallback,
    pub user_context: *mut c_void,
    pub cancel_tx: tokio::sync::oneshot::Sender<()>,
}

/// 事件类型字符串
pub fn event_type_to_string(event: &SdkEvent) -> &'static str {
    match event {
        SdkEvent::Connection(ConnectionEvent::Connected) => "connection.connected",
        SdkEvent::Connection(ConnectionEvent::Disconnected { .. }) => "connection.disconnected",
        SdkEvent::Connection(ConnectionEvent::StateChanged { .. }) => "connection.state_changed",
        SdkEvent::Connection(ConnectionEvent::KickedOff { .. }) => "connection.kicked_off",
        SdkEvent::Connection(ConnectionEvent::TokenExpired { .. }) => "connection.token_expired",
        SdkEvent::Message(MessageEvent::Received { .. }) => "message.received",
        SdkEvent::Message(MessageEvent::ReceivedBatch { .. }) => "message.received_batch",
        SdkEvent::Message(MessageEvent::SendAck { .. }) => "message.send_ack",
        SdkEvent::Message(MessageEvent::SendFailed { .. }) => "message.send_failed",
        SdkEvent::Conversation(ConversationEvent::Updated { .. }) => "conversation.updated",
        SdkEvent::Conversation(ConversationEvent::Synced { .. }) => "conversation.synced",
        SdkEvent::Sync(SyncNotify::Started) => "sync.started",
        SdkEvent::Sync(SyncNotify::Finished { .. }) => "sync.finished",
        _ => "unknown",
    }
}

/// 事件到 JSON
pub fn event_to_json(event: &SdkEvent) -> Result<String, FlareErrorCode> {
    serde_json::to_string(&EventPayload::from(event))
        .map_err(|_| FlareErrorCode::InternalError)
}
```

#### 2.3.2 事件转发

```rust
// event.rs (续)

/// 启动事件转发任务
pub fn spawn_event_forwarder(
    instance: Arc<SdkInstance>,
    subscription: Arc<EventSubscriptionInner>,
) {
    let client = instance.client.clone();
    let callback = subscription.callback;
    let user_context = subscription.user_context;
    let mut cancel_rx = subscription.cancel_tx.subscribe();
    
    instance.runtime.spawn(async move {
        // 获取事件总线
        let Ok(bus) = client.bus() else { return };
        let mut rx = bus.subscribe();
        
        loop {
            tokio::select! {
                _ = cancel_rx.recv() => break,
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            let event_type = CString::new(event_type_to_string(&event)).unwrap();
                            let event_json = match event_to_json(&event) {
                                Ok(json) => CString::new(json).unwrap().into_raw(),
                                Err(_) => continue,
                            };
                            // 调用回调
                            callback(user_context, event_type.as_ptr(), event_json);
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    }
                }
            }
        }
    });
}
```

---

## 3. API 实现

### 3.1 生命周期 API

```rust
// lifecycle.rs
use crate::{handle::*, callback::*, error::*, string::*};
use std::ffi::{c_void, CString};
use std::ptr;

/// 创建 SDK 实例
#[no_mangle]
pub extern "C" fn flare_im_new() -> FlareImHandle {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst);
    let client = IMClient::new();
    let runtime = tokio::runtime::Handle::current();
    
    let instance = Arc::new(SdkInstance {
        client,
        runtime,
        event_subscriptions: RwLock::new(Vec::new()),
    });
    
    HANDLE_TABLE.write().unwrap().insert(id, instance);
    
    FlareImHandle { id }
}

/// 释放 SDK 实例
#[no_mangle]
pub extern "C" fn flare_im_free(handle: FlareImHandle) {
    if let Ok(instance) = get_instance(handle) {
        // 取消所有事件订阅
        if let Ok(subs) = instance.event_subscriptions.write() {
            for sub in subs.drain(..) {
                let _ = sub.cancel_tx.send(());
            }
        }
        // 从句柄表移除
        HANDLE_TABLE.write().unwrap().remove(&handle.id);
    }
}

/// 初始化 SDK
#[no_mangle]
pub extern "C" fn flare_im_init(
    handle: FlareImHandle,
    config_json: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    catch_panic(|| {
        let instance = get_instance(handle)?;
        let config: SdkConfigOverlay = parse_json(config_json)?;
        
        let ctx = CallbackContext { user_context: context, callback };
        let client = instance.client.clone();
        
        instance.runtime.spawn(async move {
            let result = client.init(None, Some(config)).await
                .map_err(|e| FlareErrorCode::from(&e));
            invoke_result_callback(ctx, result);
        });
        
        Ok(())
    });
}

/// 登录
#[no_mangle]
pub extern "C" fn flare_im_login(
    handle: FlareImHandle,
    user_id: *const i8,
    token: *const i8,
    context: *mut c_void,
    callback: FlareResultCallback,
) {
    catch_panic(|| {
        let instance = get_instance(handle)?;
        let user_id = parse_string(user_id)?;
        let token = parse_string(token)?;
        
        let ctx = CallbackContext { user_context: context, callback };
        let client = instance.client.clone();
        
        instance.runtime.spawn(async move {
            use flare_im_core_sdk::client::LoginDbKind;
            
            let result = client.login(&user_id, Some(&token), LoginDbKind::Sqlite, |bus, _| {
                // 事件转发在 subscribe_events 中处理
            }).await.map_err(|e| FlareErrorCode::from(&e));
            
            invoke_result_callback(ctx, result);
        });
        
        Ok(())
    });
}

/// 是否已连接（同步）
#[no_mangle]
pub extern "C" fn flare_im_is_connected(handle: FlareImHandle) -> bool {
    get_instance(handle)
        .ok()
        .map(|i| i.client.state() == SdkState::Ready)
        .unwrap_or(false)
}
```

### 3.2 消息 API

```rust
// message.rs

/// 创建文本消息
#[no_mangle]
pub extern "C" fn flare_im_create_text_message(
    handle: FlareImHandle,
    conversation_id: *const i8,
    text: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    catch_panic(|| {
        let instance = get_instance(handle)?;
        let conversation_id = parse_string(conversation_id)?;
        let text = parse_string(text)?;
        
        let ctx = CallbackContext { user_context: context, callback };
        let client = instance.client.clone();
        
        instance.runtime.spawn(async move {
            let result = async {
                let build_api = client.message_build()?;
                let msg = build_api.create_text(&conversation_id, &text).await?;
                let json = serde_json::to_string(&msg)?;
                Ok(json)
            }.await.map_err(|e: FlareError| FlareErrorCode::from(&e));
            
            invoke_json_callback(ctx, result);
        });
        
        Ok(())
    });
}

/// 发送消息
#[no_mangle]
pub extern "C" fn flare_im_send_message(
    handle: FlareImHandle,
    message_json: *const i8,
    context: *mut c_void,
    callback: FlareStringCallback,
) {
    catch_panic(|| {
        let instance = get_instance(handle)?;
        let message: IMMessage = parse_json(message_json)?;
        
        let ctx = CallbackContext { user_context: context, callback };
        let client = instance.client.clone();
        
        instance.runtime.spawn(async move {
            let result = async {
                let api = client.message()?;
                let client_msg_id = api.send(message).await?;
                Ok(client_msg_id)
            }.await.map_err(|e: FlareError| FlareErrorCode::from(&e));
            
            invoke_string_callback(ctx, result);
        });
        
        Ok(())
    });
}

/// 获取消息列表
#[no_mangle]
pub extern "C" fn flare_im_list_messages(
    handle: FlareImHandle,
    conversation_id: *const i8,
    before_seq: u64,
    limit: i32,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    catch_panic(|| {
        let instance = get_instance(handle)?;
        let conversation_id = parse_string(conversation_id)?;
        
        let ctx = CallbackContext { user_context: context, callback };
        let client = instance.client.clone();
        
        instance.runtime.spawn(async move {
            let result = async {
                let api = client.message()?;
                let messages = api.list(&conversation_id, before_seq, limit).await?;
                let json = serde_json::to_string(&messages)?;
                Ok(json)
            }.await.map_err(|e: FlareError| FlareErrorCode::from(&e));
            
            invoke_json_callback(ctx, result);
        });
        
        Ok(())
    });
}
```

### 3.3 会话 API

```rust
// conversation.rs

/// 获取会话列表
#[no_mangle]
pub extern "C" fn flare_im_get_conversations(
    handle: FlareImHandle,
    limit: i32,
    before_id: *const i8,
    context: *mut c_void,
    callback: FlareJsonCallback,
) {
    catch_panic(|| {
        let instance = get_instance(handle)?;
        let before_id = if before_id.is_null() { None } else { Some(parse_string(before_id)?) };
        
        let ctx = CallbackContext { user_context: context, callback };
        let client = instance.client.clone();
        
        instance.runtime.spawn(async move {
            let result = async {
                let api = client.conversation()?;
                let conversations = api.list(limit, before_id.as_deref()).await?;
                let json = serde_json::to_string(&conversations)?;
                Ok(json)
            }.await.map_err(|e: FlareError| FlareErrorCode::from(&e));
            
            invoke_json_callback(ctx, result);
        });
        
        Ok(())
    });
}
```

### 3.4 事件订阅 API

```rust
// event.rs

/// 订阅事件
#[no_mangle]
pub extern "C" fn flare_im_subscribe_events(
    handle: FlareImHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> FlareEventSubscription {
    match subscribe_events_inner(handle, context, callback) {
        Ok(sub) => sub,
        Err(_) => FlareEventSubscription { id: 0 },
    }
}

fn subscribe_events_inner(
    handle: FlareImHandle,
    context: *mut c_void,
    callback: FlareEventCallback,
) -> Result<FlareEventSubscription, FlareErrorCode> {
    let instance = get_instance(handle)?;
    let id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::SeqCst);
    
    let (cancel_tx, _) = tokio::sync::oneshot::channel();
    let inner = Arc::new(EventSubscriptionInner {
        id,
        callback,
        user_context: context,
        cancel_tx,
    });
    
    // 启动事件转发
    spawn_event_forwarder(instance.clone(), inner.clone());
    
    // 注册到实例
    instance.event_subscriptions.write().unwrap().push(inner);
    
    Ok(FlareEventSubscription { id })
}

/// 取消订阅
#[no_mangle]
pub extern "C" fn flare_im_unsubscribe(subscription: FlareEventSubscription) {
    // 发送取消信号
    if let Some(tx) = SUBSCRIPTION_CANCEL_TABLE.lock().unwrap().remove(&subscription.id) {
        let _ = tx.send(());
    }
}
```

### 3.5 内存管理

```rust
// string.rs
use std::ffi::CString;

/// 释放字符串内存
#[no_mangle]
pub extern "C" fn flare_im_string_free(ptr: *const i8) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr as *mut i8)); }
    }
}

/// 释放字节数组内存
#[no_mangle]
pub extern "C" fn flare_im_bytes_free(ptr: *const u8, len: usize) {
    if !ptr.is_null() {
        unsafe { drop(Vec::from_raw_parts(ptr as *mut u8, len, len)); }
    }
}

/// 辅助：将 Rust String 转为 C 字符串（调用方需释放）
pub fn string_to_c(s: String) -> *const i8 {
    CString::new(s).unwrap().into_raw()
}

/// 辅助：解析 C 字符串为 Rust String
pub fn parse_string(ptr: *const i8) -> Result<String, FlareErrorCode> {
    if ptr.is_null() {
        return Err(FlareErrorCode::InvalidParam);
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map(|s| s.to_string())
        .map_err(|_| FlareErrorCode::InvalidParam)
}
```

### 3.6 Panic 处理

```rust
// lib.rs

/// FFI 边界 panic 捕获
pub fn catch_panic<F, R>(f: F) -> R
where
    F: FnOnce() -> Result<R, FlareErrorCode> + std::panic::UnwindSafe,
    R: Default,
{
    match std::panic::catch_unwind(f) {
        Ok(Ok(result)) => result,
        Ok(Err(code)) => {
            tracing::error!("FFI error: {:?}", code);
            R::default()
        }
        Err(e) => {
            tracing::error!("FFI panic: {:?}", e);
            R::default()
        }
    }
}
```

---

## 4. 平台适配层设计

### 4.1 Flutter 适配层

#### 4.1.1 目录结构

```
bindings/flutter/
├── pubspec.yaml
├── lib/
│   ├── flare_im_sdk.dart      # 主入口
│   ├── src/
│   │   ├── bindings.dart      # FFI 绑定（ffi.dart 生成）
│   │   ├── models.dart        # Dart 类型定义
│   │   └── errors.dart        # 错误类型
│   └── platform/
│       ├── flare_im_sdk_android.dart
│       ├── flare_im_sdk_ios.dart
│       └── flare_im_sdk_macos.dart
└── example/
    └── main.dart
```

#### 4.1.2 核心实现

```dart
// lib/src/bindings.dart
import 'dart:ffi';
import 'dart:io';

final DynamicLibrary _lib = _openLibrary();

DynamicLibrary _openLibrary() {
  if (Platform.isAndroid) return DynamicLibrary.open('libflare_im_core_sdk_ffi.so');
  if (Platform.isIOS || Platform.isMacOS) return DynamicLibrary.open('flare_im_core_sdk_ffi.framework/flare_im_core_sdk_ffi');
  if (Platform.isLinux) return DynamicLibrary.open('libflare_im_core_sdk_ffi.so');
  if (Platform.isWindows) return DynamicLibrary.open('flare_im_core_sdk_ffi.dll');
  throw UnsupportedError('Unsupported platform');
}

// FFI 函数绑定
final int Function() _flareImNew = 
    _lib.lookupFunction<Uint64 Function(), int Function()>('flare_im_new');

final void Function(int, Pointer<Utf8>, Pointer<Void>, Pointer<NativeFunction>) _flareImInit =
    _lib.lookupFunction<Void Function(Uint64, Pointer<Utf8>, Pointer<Void>, Pointer<NativeFunction>),
                       void Function(int, Pointer<Utf8>, Pointer<Void>, Pointer<NativeFunction>)>('flare_im_init');

// ... 其他函数绑定
```

```dart
// lib/flare_im_sdk.dart
import 'dart:async';
import 'dart:convert';
import 'dart:ffi';

class FlareImSdk {
  final int _handle;
  final StreamController<SdkEvent> _events = StreamController.broadcast();
  int? _subscriptionId;
  
  FlareImSdk._(this._handle);
  
  /// 初始化 SDK
  static Future<FlareImSdk> init(SdkConfig config) async {
    final handle = _flareImNew();
    final completer = Completer<void>();
    
    _flareImInit(
      handle,
      config.toJson().toNativeUtf8(),
      nullptr,
      Pointer.fromFunction<_ResultCallback>(_onInitResult, completer),
    );
    
    await completer.future;
    final sdk = FlareImSdk._(handle);
    sdk._subscribeEvents();
    return sdk;
  }
  
  /// 事件流
  Stream<SdkEvent> get events => _events.stream;
  
  /// 登录
  Future<void> login(String userId, String token) {
    final completer = Completer<void>();
    _flareImLogin(_handle, userId.toNativeUtf8(), token.toNativeUtf8(), 
                  nullptr, Pointer.fromFunction(_onResult, completer));
    return completer.future;
  }
  
  /// 创建文本消息
  Future<IMMessage> createText(String conversationId, String text) {
    final completer = Completer<String>();
    _flareImCreateTextMessage(_handle, conversationId.toNativeUtf8(), text.toNativeUtf8(),
                              nullptr, Pointer.fromFunction(_onJsonResult, completer));
    return completer.future.then((json) => IMMessage.fromJson(jsonDecode(json)));
  }
  
  /// 发送消息
  Future<String> send(IMMessage message) {
    final completer = Completer<String>();
    _flareImSendMessage(_handle, jsonEncode(message).toNativeUtf8(),
                        nullptr, Pointer.fromFunction(_onStringResult, completer));
    return completer.future;
  }
  
  void _subscribeEvents() {
    _subscriptionId = _flareImSubscribeEvents(
      _handle,
      nullptr,
      Pointer.fromFunction(_onEvent, this),
    );
  }
  
  /// 释放资源
  void dispose() {
    if (_subscriptionId != null) _flareImUnsubscribe(_subscriptionId!);
    _flareImFree(_handle);
    _events.close();
  }
}
```

### 4.2 Android 适配层

#### 4.2.1 目录结构

```
bindings/android/
├── build.gradle.kts
├── src/main/
│   ├── AndroidManifest.xml
│   ├── kotlin/com/flare/im/
│   │   ├── FlareImSdk.kt
│   │   ├── Models.kt
│   │   ├── Errors.kt
│   │   └── Native.kt
│   └── jniLibs/
│       ├── arm64-v8a/libflare_im_core_sdk_ffi.so
│       ├── armeabi-v7a/libflare_im_core_sdk_ffi.so
│       └── x86_64/libflare_im_core_sdk_ffi.so
```

#### 4.2.2 核心实现

```kotlin
// FlareImSdk.kt
package com.flare.im

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

class FlareImSdk private constructor(private val handle: Long) {
    private val eventFlow = MutableSharedFlow<SdkEvent>()
    
    companion object {
        init { System.loadLibrary("flare_im_core_sdk_ffi") }
        
        suspend fun init(config: SdkConfig): FlareImSdk = suspendCancellableCoroutine { cont ->
            val handle = nativeNew()
            nativeInit(handle, config.toJson(), object : ResultCallback {
                override fun onResult(code: Int, message: String?) {
                    if (code == 0) cont.resume(FlareImSdk(handle).apply { subscribeEvents() })
                    else cont.resumeWithException(SdkException(code, message ?: "Unknown error"))
                }
            })
        }
    }
    
    val events: Flow<SdkEvent> get() = eventFlow
    
    suspend fun login(userId: String, token: String) = suspendCancellableCoroutine<Unit> { cont ->
        nativeLogin(handle, userId, token, object : ResultCallback {
            override fun onResult(code: Int, message: String?) {
                if (code == 0) cont.resume(Unit)
                else cont.resumeWithException(SdkException(code, message ?: "Unknown error"))
            }
        })
    }
    
    suspend fun createText(conversationId: String, text: String): IMMessage = 
        suspendCancellableCoroutine { cont ->
            nativeCreateTextMessage(handle, conversationId, text, object : JsonCallback {
                override fun onResult(code: Int, json: String?) {
                    if (code == 0 && json != null) cont.resume(IMMessage.fromJson(json))
                    else cont.resumeWithException(SdkException(code, json ?: "Unknown error"))
                }
            })
        }
    
    private fun subscribeEvents() {
        nativeSubscribeEvents(handle, object : EventCallback {
            override fun onEvent(eventType: String, eventJson: String) {
                eventFlow.tryEmit(SdkEvent.from(eventType, eventJson))
            }
        })
    }
    
    fun dispose() {
        nativeFree(handle)
    }
    
    // Native 方法
    private external fun nativeNew(): Long
    private external fun nativeInit(handle: Long, configJson: String, callback: ResultCallback)
    private external fun nativeLogin(handle: Long, userId: String, token: String, callback: ResultCallback)
    private external fun nativeCreateTextMessage(handle: Long, conversationId: String, text: String, callback: JsonCallback)
    private external fun nativeSubscribeEvents(handle: Long, callback: EventCallback)
    private external fun nativeFree(handle: Long)
}

// JNI 回调接口
interface ResultCallback { fun onResult(code: Int, message: String?) }
interface JsonCallback { fun onResult(code: Int, json: String?) }
interface EventCallback { fun onEvent(eventType: String, eventJson: String) }
```

### 4.3 iOS 适配层

#### 4.3.1 目录结构

```
bindings/ios/
├── Package.swift
├── Sources/
│   ├── FlareImSdk/
│   │   ├── FlareImSdk.swift
│   │   ├── Models.swift
│   │   └── Errors.swift
│   └── flare_im_core_sdk_ffi.xcframework/
└── Tests/
    └── FlareImSdkTests/
```

#### 4.3.2 核心实现

```swift
// FlareImSdk.swift
import Foundation
import Combine

public class FlareImSdk {
    private let handle: UInt64
    private let eventSubject = PassthroughSubject<SdkEvent, Never>()
    private var subscription: UInt64 = 0
    
    public var events: AnyPublisher<SdkEvent, Never> { eventSubject.eraseToAnyPublisher() }
    
    private init(handle: UInt64) {
        self.handle = handle
    }
    
    public static func initialize(config: SdkConfig) async throws -> FlareImSdk {
        let handle = flare_im_new()
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            let callback = ResultCallback { code, message in
                if code == 0 { cont.resume(returning: ()) }
                else { cont.resume(throwing: SdkException(code: code, message: message ?? "Unknown")) }
            }
            flare_im_init(handle, config.toJson(), nil, callback)
        }
        let sdk = FlareImSdk(handle: handle)
        sdk.subscribeEvents()
        return sdk
    }
    
    public func login(userId: String, token: String) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            let callback = ResultCallback { code, message in
                if code == 0 { cont.resume(returning: ()) }
                else { cont.resume(throwing: SdkException(code: code, message: message ?? "Unknown")) }
            }
            flare_im_login(handle, userId, token, nil, callback)
        }
    }
    
    public func createText(conversationId: String, text: String) async throws -> IMMessage {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<IMMessage, Error>) in
            let callback = JsonCallback { code, json in
                if code == 0, let json = json, let msg = try? IMMessage.fromJson(json) {
                    cont.resume(returning: msg)
                } else {
                    cont.resume(throwing: SdkException(code: code, message: "Failed to create message"))
                }
            }
            flare_im_create_text_message(handle, conversationId, text, nil, callback)
        }
    }
    
    private func subscribeEvents() {
        let callback = EventCallback { [weak self] eventType, eventJson in
            if let event = SdkEvent.from(type: eventType, json: eventJson) {
                self?.eventSubject.send(event)
            }
        }
        subscription = flare_im_subscribe_events(handle, nil, callback)
    }
    
    deinit {
        if subscription != 0 { flare_im_unsubscribe(subscription) }
        flare_im_free(handle)
    }
}
```

---

## 5. 构建与集成

### 5.1 Rust 构建

```toml
# bindings/c/Cargo.toml
[package]
name = "flare-im-core-sdk-ffi"
version = "0.3.0"
edition = "2024"
rust-version = "1.94.0"

[lib]
name = "flare_im_core_sdk_ffi"
crate-type = ["cdylib", "staticlib"]

[dependencies]
flare-im-core-sdk = { path = "../.." }
flare-im-core-sdk-storage-sqlite = { path = "../../storage/sqlite" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
serde_json = "1.0"
lazy_static = "1.4"
tracing = "0.1"

[build-dependencies]
cbindgen = "0.26"
```

### 5.2 cbindgen 配置

```toml
# bindings/c/cbindgen.toml
language = "C"
header = """
/*
 * Flare IM Core SDK - C ABI Header
 * Auto-generated by cbindgen. DO NOT EDIT.
 */
"""
include_guard = "FLARE_IM_CORE_SDK_FFI_H"
pragma_once = true

[export]
include = [
    "FlareImHandle",
    "FlareEventSubscription",
    "FlareErrorCode",
    "flare_im_*",
]

[parse]
parse_deps = false
include = ["flare-im-core-sdk-ffi"]

[enum]
prefix_with_name = true
```

### 5.3 构建脚本

```rust
// bindings/c/build.rs
fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let output_file = std::path::PathBuf::from(&crate_dir)
        .parent().unwrap()
        .parent().unwrap()
        .join("target")
        .join("flare_im_core_sdk_ffi.h");
    
    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(cbindgen::Config::from_file(&format!("{}/cbindgen.toml", crate_dir)).unwrap())
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(&output_file);
    
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
```

---

## 6. 测试策略

### 6.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_handle_lifecycle() {
        let handle = flare_im_new();
        assert!(handle.id > 0);
        flare_im_free(handle);
    }
    
    #[test]
    fn test_error_code_mapping() {
        let err = FlareError::localized(ErrorCode::NotConnected, "test");
        let code: FlareErrorCode = (&err).into();
        assert_eq!(code, FlareErrorCode::NotConnected);
    }
    
    #[tokio::test]
    async fn test_event_subscription() {
        let handle = flare_im_new();
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        let subscription = flare_im_subscribe_events(
            handle,
            Box::into_raw(Box::new(tx)) as *mut c_void,
            event_callback,
        );
        
        // ... 测试事件推送
        
        flare_im_unsubscribe(subscription);
        flare_im_free(handle);
    }
}
```

### 6.2 集成测试

```rust
#[tokio::test]
async fn test_full_lifecycle() {
    let handle = flare_im_new();
    
    // 初始化
    let config = r#"{"data_url": "file:///tmp/test"}"#;
    let result = init_sync(handle, config);
    assert_eq!(result, FlareErrorCode::Ok);
    
    // 登录
    let result = login_sync(handle, "user1", "token1");
    assert_eq!(result, FlareErrorCode::Ok);
    
    // 发送消息
    let msg = create_text_sync(handle, "conv1", "hello");
    let client_msg_id = send_sync(handle, &msg);
    assert!(!client_msg_id.is_empty());
    
    // 清理
    flare_im_free(handle);
}
```

---

## 7. 性能优化

### 7.1 JSON 序列化优化

- 使用 `serde_json::to_string` 预分配缓冲区
- 对于高频事件，考虑使用 `smallvec` 减少分配

### 7.2 回调调度优化

- 使用 `tokio::spawn` 避免阻塞主线程
- 批量事件合并推送，减少回调次数

### 7.3 内存管理优化

- 使用对象池复用 `CString` 分配
- 避免不必要的 `Arc` 克隆

---

## 8. 安全考虑

### 8.1 FFI 边界安全

- 所有 `extern "C"` 函数必须使用 `catch_panic`
- 禁止在回调中 panic

### 8.2 内存安全

- 传出字符串必须由调用方释放
- 句柄释放后不可再使用

### 8.3 线程安全

- 句柄表使用 `RwLock` 保护
- 回调可在任意线程执行，平台侧需处理
