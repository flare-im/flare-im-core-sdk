# C ABI FFI Bindings

提供 C 兼容的 API，供各平台自动生成绑定。

## 设计原则

1. **最小化编码**：只做必要的类型转换
2. **完全自动化**：各平台绑定自动生成
3. **性能最优**：直接调用，无序列化开销

## 使用方式

1. 使用 `cbindgen` 生成 C 头文件
2. 各平台从 C 头文件自动生成绑定
3. 零编码，完全自动化

## 构建

```bash
# 构建库
cargo build -p flare-im-core-sdk-ffi --release

# 生成 C 头文件（自动在 build.rs 中执行）
cargo build -p flare-im-core-sdk-ffi
# 头文件位置: target/flare_im_core_sdk_ffi.h
```

## API 说明

所有 API 都使用 callback 模式处理异步结果：

```c
typedef void (*Callback)(void* user_data, const char* result, const char* error);
```

### 主要函数

- `flare_im_sdk_new` - 创建 SDK 实例
- `flare_im_sdk_login` - 登录
- `flare_im_sdk_create_text_message` - 创建文本消息
- `flare_im_sdk_send_message` - 发送消息
- `flare_im_sdk_get_conversations` - 获取会话列表
- `flare_im_sdk_get_messages` - 获取消息列表
- `flare_im_sdk_free` - 释放 SDK 实例

## 安全性说明

此模块包含 FFI 代码，虽然使用了 `#[unsafe(no_mangle)]` 和原始指针，
但所有公共 API 都是安全的。所有 unsafe 操作都封装在 `safe` 模块中。
