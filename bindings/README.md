# Bindings 目录

此目录包含 Flare IM Core SDK 的各平台绑定层。

## 目录结构

```
bindings/
├── c/              # C ABI 绑定（cbindgen）
│   ├── Cargo.toml
│   ├── cbindgen.toml
│   ├── build.rs
│   └── src/
│       ├── lib.rs
│       ├── client.rs
│       ├── safe.rs
│       ├── types.rs
│       └── error.rs
│
├── uniffi/         # 已废弃（见 uniffi/README.md）；客户端统一走 bindings/c
│
└── tauri/          # Tauri 桌面壳（内部工具）
```

## C ABI 绑定 (`bindings/c`)

提供 C 兼容的 API，供各平台自动生成绑定。

### 构建

```bash
# 构建 C ABI 库
cargo build -p flare-im-core-sdk-ffi --release

# 生成 C 头文件
cargo build -p flare-im-core-sdk-ffi
# 头文件将生成在: target/flare_im_core_sdk_ffi.h
```

### 使用

C ABI 绑定提供了以下主要函数：

- `flare_im_sdk_new` - 创建 SDK 实例
- `flare_im_sdk_login` - 登录
- `flare_im_sdk_create_text_message` - 创建文本消息
- `flare_im_sdk_send_message` - 发送消息
- `flare_im_sdk_get_conversations` - 获取会话列表
- `flare_im_sdk_get_messages` - 获取消息列表
- `flare_im_sdk_free` - 释放 SDK 实例

所有函数都使用 callback 模式处理异步结果。

## 架构设计

- **Core SDK** (`../src/`) - Rust 核心实现，返回领域模型
- **FFI 层** (`bindings/c/`) - 类型转换层，负责：
  - JSON ↔ 领域模型转换
  - C 类型 ↔ Rust 类型转换
  - Callback 管理
  - 生命周期管理

## 客户端 SDK

多端 L2/L3 封装见 monorepo 根目录 [`flare-im-core-client-sdk`](../../flare-im-core-client-sdk)。

## 未来计划

- [ ] Android / iOS / 鸿蒙 L3 包（均桥接本目录 C ABI）
- [x] Flutter Dart 包（`flare-im-core-client-sdk/packages/dart/flare_im_sdk`）
