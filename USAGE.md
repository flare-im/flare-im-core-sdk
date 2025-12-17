# Flare IM Core SDK 使用指南

> 更新日期：2025-01-XX  
> SDK 版本：0.1.0

## 一、概述

Flare IM Core SDK 是一个跨平台的即时通讯客户端 SDK，支持：
- ✅ **Web** (WASM)
- ✅ **Android** (通过 C ABI + JNI)
- ✅ **iOS** (通过 C ABI + Swift)
- ✅ **Desktop** (Tauri/原生 Rust)

## 二、架构设计

### 2.1 轻量级适配层

SDK 采用 **C ABI + 自动绑定生成** 的轻量级方案：

```
核心 SDK (Rust)
    ↓
C ABI 包装层 (约 150 行，一次性)
    ↓
自动生成的 C 头文件 (cbindgen)
    ↓
各平台自动生成绑定 (零编码)
    ↓
平台应用代码
```

**优势**：
- ✅ **编码工作量减少 90%+**：从 1500+ 行 → 150 行
- ✅ **完全自动化**：各平台绑定自动生成
- ✅ **性能最优**：直接调用，无序列化开销

---

## 三、Rust 原生使用（推荐）

### 3.1 基本使用

```rust
use flare_im_core_sdk::{FlareIMClient, ClientConfig, ClientConfigBuilder};
use flare_core::common::config_types::TransportProtocol;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 创建配置
    let config = ClientConfig::builder()
        .server_url("wss://im.example.com")
        .user_id("user_123")
        .device_id("device_456")
        .token("your_token")
        .protocols(vec![
            TransportProtocol::QUIC,
            TransportProtocol::WebSocket,
        ])
        .build()?;

    // 2. 创建客户端
    let client = FlareIMClient::new(config).await?;

    // 3. 登录
    let login_result = client.login("user_123", "your_token").await?;
    println!("登录成功: {:?}", login_result);

    // 4. 发送消息
    let message_id = client.send_message(
        "session_123",
        flare_proto::MessageContent {
            content: Some(flare_proto::flare::common::v1::message_content::Content::Text(
                flare_proto::TextContent {
                    text: "Hello, World!".to_string(),
                    mentions: vec![],
                }
            )),
        },
    ).await?;
    println!("消息已发送: {}", message_id);

    // 5. 获取会话列表
    let sessions = client.get_sessions(flare_im_core_sdk::SessionFilter::default()).await?;
    println!("会话数量: {}", sessions.len());

    // 6. 获取消息列表
    let messages = client.get_messages("session_123", 50, None).await?;
    println!("消息数量: {}", messages.len());

    Ok(())
}
```

### 3.2 事件监听

```rust
use flare_im_core_sdk::infrastructure::event::{Event, ConnectionEvent, MessageEvent};

// 订阅事件
let event_bus = client.event_bus();
let mut rx = event_bus.subscribe();

tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
        match event {
            Event::Connection(cev) => {
                match cev {
                    ConnectionEvent::Connected { .. } => {
                        println!("已连接");
                    }
                    ConnectionEvent::Disconnected => {
                        println!("已断开");
                    }
                    ConnectionEvent::Authenticated => {
                        println!("已认证");
                    }
                    _ => {}
                }
            }
            Event::Message(mev) => {
                match mev {
                    MessageEvent::MessageReceived { message_id, session_id } => {
                        println!("收到消息: {} in {}", message_id, session_id);
                    }
                    MessageEvent::MessageSent { message_id, .. } => {
                        println!("消息已发送: {}", message_id);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
});
```

---

## 四、Android 使用（C ABI + JNI）

### 4.1 生成 C 头文件

```bash
cd flare-im-core-sdk
cargo build --features ffi --release
# 生成的文件：target/flare_im_core_sdk.h
```

### 4.2 生成 JNI 绑定

**方案一：使用 javah（推荐）**

```bash
# 从 C 头文件生成 JNI 绑定
javac -h jni/ flare_im_core_sdk.h
```

**方案二：使用 jni-rs 自动生成**

```rust
// 在 Android 项目中添加 jni-rs 依赖
// 自动生成 JNI 绑定代码
```

### 4.3 Android 使用示例

```kotlin
// FlareIMClient.kt (自动生成或手写包装)
class FlareIMClient {
    companion object {
        init {
            System.loadLibrary("flare_im_core_sdk")
        }
    }
    
    external fun new(configJson: String, callback: (String?, String?) -> Unit)
    external fun login(handle: Long, userId: String, token: String, callback: (String?, String?) -> Unit)
    external fun sendMessage(handle: Long, sessionId: String, text: String, callback: (String?, String?) -> Unit)
    external fun getSessions(handle: Long, filterJson: String?, callback: (String?, String?) -> Unit)
    external fun getMessages(handle: Long, sessionId: String, limit: Int, cursor: String?, callback: (String?, String?) -> Unit)
    external fun free(handle: Long)
}

// 使用示例
class ChatViewModel : ViewModel() {
    private var clientHandle: Long? = null
    
    fun initialize() {
        val config = """
        {
            "server_url": "wss://im.example.com",
            "user_id": "user_123",
            "device_id": "device_456",
            "device_platform": "Android"
        }
        """.trimIndent()
        
        FlareIMClient().new(config) { result, error ->
            if (error != null) {
                Log.e("FlareIM", "初始化失败: $error")
            } else {
                val handle = JSONObject(result).getLong("handle")
                clientHandle = handle
                Log.d("FlareIM", "初始化成功: handle=$handle")
            }
        }
    }
    
    fun login(userId: String, token: String) {
        clientHandle?.let { handle ->
            FlareIMClient().login(handle, userId, token) { result, error ->
                if (error != null) {
                    Log.e("FlareIM", "登录失败: $error")
                } else {
                    Log.d("FlareIM", "登录成功: $result")
                }
            }
        }
    }
    
    fun sendMessage(sessionId: String, text: String) {
        clientHandle?.let { handle ->
            FlareIMClient().sendMessage(handle, sessionId, text) { result, error ->
                if (error != null) {
                    Log.e("FlareIM", "发送失败: $error")
                } else {
                    val messageId = JSONObject(result).getString("message_id")
                    Log.d("FlareIM", "消息已发送: $messageId")
                }
            }
        }
    }
}
```

---

## 五、iOS 使用（C ABI + Swift）

### 5.1 生成 C 头文件

```bash
cd flare-im-core-sdk
cargo build --features ffi --release --target aarch64-apple-ios
# 生成的文件：target/flare_im_core_sdk.h
```

### 5.2 Swift 使用示例

**方案一：Swift 5.9+ 直接导入 C 头文件**

```swift
// 在 Xcode 项目中添加 C 头文件
// Swift 会自动生成绑定

import Foundation

class FlareIMClient {
    private var handle: UInt64?
    
    func initialize(config: [String: Any]) {
        let configJson = try! JSONSerialization.data(withJSONObject: config)
        let configString = String(data: configJson, encoding: .utf8)!
        
        // 调用 C ABI
        flare_im_client_new(
            configString,
            { userData, result, error in
                // 处理回调
            },
            nil
        )
    }
    
    func login(userId: String, token: String) {
        guard let handle = handle else { return }
        
        flare_im_client_login(
            handle,
            userId,
            token,
            { userData, result, error in
                // 处理回调
            },
            nil
        )
    }
}
```

**方案二：使用 swift-bridge 自动生成**

```rust
// 在 Rust 代码中使用 swift-bridge
#[swift_bridge::bridge]
mod ffi {
    extern "Rust" {
        type FlareIMClient;
        
        #[swift_bridge(init)]
        fn new(config: String) -> FlareIMClient;
        
        fn login(&self, userId: String, token: String) -> String;
    }
}
```

---

## 六、Web 使用（WASM）

### 6.1 构建 WASM

```bash
cd flare-im-core-sdk
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/flare_im_core_sdk.wasm \
    --out-dir pkg \
    --target web
```

### 6.2 Web 使用示例

```typescript
// 导入 WASM 模块
import init, { FlareIMClient } from './pkg/flare_im_core_sdk.js';

async function main() {
    // 初始化 WASM
    await init();
    
    // 创建配置
    const config = {
        server_url: "wss://im.example.com",
        user_id: "user_123",
        device_id: "device_456",
        device_platform: "Web"
    };
    
    // 创建客户端
    const client = new FlareIMClient();
    await client.initialize(config);
    
    // 登录
    const loginResult = await client.login("user_123", "token");
    console.log("登录成功:", loginResult);
    
    // 发送消息
    const messageId = await client.sendMessage("session_123", "Hello, World!");
    console.log("消息已发送:", messageId);
    
    // 获取会话列表
    const sessions = await client.getSessions(null, null);
    console.log("会话列表:", sessions);
    
    // 获取消息列表
    const messages = await client.getMessages("session_123", 50, null);
    console.log("消息列表:", messages);
}

main();
```

---

## 七、配置说明

### 7.1 ClientConfig JSON 格式

```json
{
    "server_url": "wss://im.example.com",
    "user_id": "user_123",
    "device_id": "device_456",
    "device_platform": "Android",
    "token": "your_token",
    "connect_timeout": 15,
    "heartbeat_interval": 30,
    "auto_reconnect": true,
    "max_reconnect_attempts": 10,
    "protocols": ["QUIC", "WebSocket"],
    "protocol_urls": {
        "WebSocket": "ws://im.example.com:60051",
        "QUIC": "quic://im.example.com:60052"
    }
}
```

### 7.2 SessionFilter JSON 格式

```json
{
    "unread_only": false,
    "limit": 50,
    "offset": 0
}
```

---

## 八、错误处理

### 8.1 错误格式

所有错误通过回调函数的 `error_json` 参数返回：

```json
{
    "error": "错误描述",
    "code": 1001,
    "type": "Connection"
}
```

### 8.2 错误类型

- `Connection`: 连接错误
- `Authentication`: 认证错误
- `Message`: 消息错误
- `Storage`: 存储错误
- `Sync`: 同步错误
- `Config`: 配置错误
- `Internal`: 内部错误

---

## 九、最佳实践

### 9.1 客户端生命周期管理

```rust
// ✅ 正确：使用 Arc 共享客户端
let client = Arc::new(FlareIMClient::new(config).await?);

// ✅ 正确：在多个任务中共享
let client_clone = Arc::clone(&client);
tokio::spawn(async move {
    client_clone.send_message(...).await;
});
```

### 9.2 事件处理

```rust
// ✅ 正确：在独立任务中处理事件
let mut rx = client.event_bus().subscribe();
tokio::spawn(async move {
    while let Ok(event) = rx.recv().await {
        // 处理事件
    }
});
```

### 9.3 错误处理

```rust
// ✅ 正确：使用 ? 操作符传播错误
match client.login(user_id, token).await {
    Ok(result) => println!("登录成功: {:?}", result),
    Err(e) => {
        eprintln!("登录失败: {}", e);
        // 处理错误
    }
}
```

---

## 十、常见问题

### Q1: 如何在不同平台使用？

**A**: 
- **Rust 原生**：直接使用 `FlareIMClient`
- **Android**：使用 C ABI + JNI 绑定（自动生成）
- **iOS**：使用 C ABI + Swift 绑定（自动生成）
- **Web**：使用 WASM 绑定（wasm-bindgen）

### Q2: 如何生成各平台绑定？

**A**: 
1. 构建 SDK 时启用 `ffi` feature：`cargo build --features ffi`
2. C 头文件自动生成在 `target/flare_im_core_sdk.h`
3. 各平台从 C 头文件自动生成绑定（使用平台工具）

### Q3: 如何处理异步操作？

**A**: 
- **Rust**：直接使用 `async/await`
- **Android/iOS/Web**：使用回调函数（C ABI 自动处理异步）

### Q4: 如何扩展 SDK 功能？

**A**: 参考 [扩展指南](./EXTENSION_GUIDE.md)

---

## 十一、参考资源

- [架构设计文档](./ARCHITECTURE_ANALYSIS.md)
- [扩展指南](./EXTENSION_GUIDE.md)
- [API 文档](https://docs.rs/flare-im-core-sdk)

