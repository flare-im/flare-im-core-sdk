# Flare IM Core SDK 最新产物同步命令

> 目标示例应用:
>
> - `/Users/hg/workspace/flare/flare-im/flare-im-core-client-sdk/examples/flare-core-web-app`
> - `/Users/hg/workspace/flare/flare-im/flare-im-core-client-sdk/examples/flare-core-flutter-app`
>
> 原则:开发阶段不保留兼容分支,以最新 `flare-im-core-sdk` 生成产物和 core-owned contract 为准。

## Web App

```bash
# 1. 生成最新 core contract / sdk-spec / TypeScript SDK 代码
cd /Users/hg/workspace/flare/flare-im/flare-im-core-sdk
rtk cargo xtask codegen
rtk cargo xtask codegen-check

# 2. 构建并放置最新 Web WASM 产物
rtk cargo xtask build wasm

# 3. 安装依赖并验证 Web 示例应用
cd /Users/hg/workspace/flare/flare-im/flare-im-core-client-sdk/examples/flare-core-web-app
rtk npm install
rtk npm run typecheck
rtk npm test -- src/shared/testing/smoke.test.ts
rtk npm run test:e2e

# 4. 启动 Web 示例应用
rtk npm run dev
```

## Flutter App

```bash
# 1. 生成最新 core contract / sdk-spec / Flutter SDK 代码
cd /Users/hg/workspace/flare/flare-im/flare-im-core-sdk
rtk cargo xtask codegen
rtk cargo xtask codegen-check

# 2. 构建并放置最新 host FFI + iOS simulator FFI 产物
rtk cargo xtask build host ios-sim

# 3. 验证 Flutter SDK 包
cd /Users/hg/workspace/flare/flare-im/flare-im-core-client-sdk/packages/flare-core-flutter-sdk
rtk dart analyze

# 4. 安装依赖并验证 Flutter 示例应用
cd /Users/hg/workspace/flare/flare-im/flare-im-core-client-sdk/examples/flare-core-flutter-app
rtk flutter pub get
rtk flutter analyze

# 5. 启动 Flutter iOS 模拟器示例应用
rtk flutter run -d "iPhone 17 Pro"
```

## Web + Flutter 一次性同步

```bash
cd /Users/hg/workspace/flare/flare-im/flare-im-core-sdk
rtk cargo xtask codegen
rtk cargo xtask codegen-check
rtk cargo xtask build host wasm ios-sim
```

## 产物落点核对

```bash
cd /Users/hg/workspace/flare/flare-im/flare-im-core-client-sdk

rtk find native/artifacts examples/flare-core-flutter-app \
  -name 'libflare_im_core_sdk_ffi*' \
  -o -name 'flare_im_core_sdk_ffi.h' \
  -o -name 'flare_im_core_sdk*.wasm' \
  -o -name 'flare_im_core_sdk*.js'
```
