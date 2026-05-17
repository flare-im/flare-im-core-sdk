# Flare IM SDK - C ABI Bindings

跨平台 C ABI SDK,支持 iOS、Android、Flutter、鸿蒙、C/C++、Node、Unity。

## 架构

- **Rust 内部复杂,C ABI 外部简单**
- **所有对象通过 handle 管理**
- **异步 API 使用 callback**
- **统一 error code 错误模型**
- **显式内存管理**

## 文件结构

```
bindings/c/
├── Cargo.toml          # 项目配置
├── cbindgen.toml       # 头文件生成配置
├── build.rs            # 构建脚本
├── ARCHITECTURE.md     # 架构设计文档
└── src/
    ├── lib.rs           # 入口
    ├── types.rs         # C ABI 类型定义
    ├── registry.rs      # 句柄注册表
    ├── error_convert.rs # 错误转换
    ├── helpers.rs       # 辅助工具
    ├── executor.rs      # 回调执行器
    └── lifecycle.rs     # 生命周期 API
```

## API

### 生命周期

```c
// 创建/释放
FlareHandle flare_sdk_create();
void flare_sdk_release(FlareHandle handle);

// 初始化/登录/登出
int32_t flare_sdk_init(FlareHandle handle, const char* config_json, void* context, FlareResultCallback callback);
int32_t flare_sdk_login(FlareHandle handle, const char* user_id, const char* token, const char* store_config_json, void* context, FlareResultCallback callback);
int32_t flare_sdk_logout(FlareHandle handle, void* context, FlareResultCallback callback);

// 同步查询
FlareString flare_sdk_version();
bool flare_sdk_is_connected(FlareHandle handle);

// 当前用户 ID（异步，走 callback）
int32_t flare_sdk_current_user_id(FlareHandle handle, void* context, FlareResultCallback callback);

// 开发用 JWT（同步，不依赖 handle；`secret`/`issuer` 可为 NULL 或空串用内置默认；`tenant_id` 可为 NULL；`ttl_secs==0` 表示 3600；须 flare_string_free）
FlareString flare_sdk_generate_test_token(const char* secret, const char* issuer, const char* user_id, const char* tenant_id, uint64_t ttl_secs);
```

### 内存管理

```c
void flare_string_free(FlareString s);
void flare_bytes_free(FlareBytes b);
void flare_error_free(FlareError e);
```

## 构建

```bash
cargo build -p flare-im-core-sdk-ffi --release
```

### Flutter 示例（`examples/flare-core-flutter`）一键同步产物

在 `bindings/c` 目录执行 `make flutter-sync`，会把 C ABI 产物同步到 Flutter 示例工程对应目录。

完整打包说明见 [`docs/flutter-packaging.md`](docs/flutter-packaging.md)。

## 线程安全

- callback 可能来自任意线程
- 禁止在 callback 中阻塞或持锁
- 所有 API 线程安全

## 平台支持

- iOS (Swift/Objective-C)
- Android (Kotlin/Java)
- Flutter (Dart)：完整示例在 monorepo **`examples/flare-core-flutter/`**；在 **`bindings/c`** 执行 **`make flutter-sync`** 将 dylib / `.a` / `.so` 拷入该工程对应目录（见上节）
- 鸿蒙 (ArkTS/C++)
- C/C++
- Node.js (N-API)
- Unity (C# P/Invoke)
