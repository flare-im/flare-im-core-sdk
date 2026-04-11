# Flare IM Flutter 应用

基于 Flutter + Rust 的高性能跨平台即时通讯应用。

## 项目概述

本项目使用 Flutter 框架开发跨平台 IM 应用,通过 FFI (Foreign Function Interface) 调用 Rust SDK 的 C 语言绑定层,实现高性能的即时通讯功能。

### 技术栈

- **UI 框架**: Flutter 3.16+
- **状态管理**: Riverpod 2.4+
- **FFI 绑定**: dart:ffi
- **序列化**: json_serializable + freezed
- **路由**: go_router 13+
- **底层 SDK**: flare-im-core-sdk (Rust C ABI)

### 架构设计

项目采用 DDD (领域驱动设计) 分层架构。**开发与通信约定**（下行 EventBus、上行 `ImOutboundFacade`、分层禁忌等）见 [docs/DEVELOPMENT_SPEC.md](docs/DEVELOPMENT_SPEC.md)。

```
lib/
├── domain/              # 领域层 (纯业务逻辑)
│   ├── entities/        # 实体
│   ├── value_objects/   # 值对象
│   ├── repositories/    # 仓库接口
│   └── events/          # 领域事件
│
├── application/         # 应用层 (编排)
│   ├── services/        # 应用服务
│   ├── usecases/        # 用例
│   └── providers/       # Riverpod Providers
│
├── infrastructure/      # 基础设施层
│   ├── ffi/             # FFI 绑定层
│   │   ├── bindings/    # 自动生成的绑定
│   │   ├── wrapper/     # 手动封装
│   │   └── types/       # FFI 类型转换
│   ├── repositories/    # 仓库实现
│   ├── storage/         # 存储实现
│   └── platform/        # 平台适配
│
└── interface/           # 接口层 (UI)
    ├── screens/         # 页面
    ├── widgets/         # 组件
    └── router/          # 路由配置
```

## 核心功能

### 已实现

- ✅ 项目基础架构搭建
- ✅ DDD 分层目录结构
- ✅ FFI 绑定层实现
  - 动态库加载器 (跨平台支持)
  - C 函数签名定义
  - 异步回调封装
  - SDK Wrapper
- ✅ 领域层实现
  - 实体: Conversation, Message, User
  - 值对象: ConversationType, MessageContent, ConnectionState
  - 仓库接口: IAuthRepository, IConversationRepository, IMessageRepository
  - 领域事件: MessageEvent, ConversationEvent

### 待实现

- ⏳ 应用层服务实现
- ⏳ 仓库实现 (基于 FFI)
- ⏳ UI 组件开发
- ⏳ 状态管理集成
- ⏳ 路由配置

## 快速开始

### 环境要求

- Flutter SDK 3.16+
- Rust 工具链 (stable)
- Xcode (iOS/macOS)
- Android Studio (Android)
- Visual Studio (Windows)

### 安装依赖

```bash
flutter pub get
```

### 运行应用

**iOS**：需本机安装 Rust；首次构建前建议 `rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`。Xcode 会通过 **Build Rust FFI** 脚本编译 `bindings/c` 并 `-force_load` 链入 Runner（详见 `ios/FFI/README.md`）。若出现 `dlsym ... flare_sdk_create ... symbol not found`，即静态库未链入或未 `force_load`。

```bash
# iOS
flutter run -d ios

# Android
flutter run -d android

# macOS
flutter run -d macos

# Windows
flutter run -d windows

# Linux
flutter run -d linux
```

## FFI 绑定使用示例

### 1. 初始化 SDK

```dart
import 'package:flare_im/infrastructure/ffi/wrapper/sdk_wrapper.dart';

final sdk = SdkWrapper();
await sdk.init(SdkConfig(
  serverUrl: 'https://im.example.com',
  logLevel: 'debug',
));
```

### 2. 用户登录

```dart
await sdk.login('user123', 'your_token_here');
```

### 3. 获取会话列表

```dart
final conversations = await sdk.getConversations();
for (final conv in conversations) {
  print('会话: ${conv['display_name']}');
}
```

### 4. 发送消息

```dart
// 创建文本消息
final message = await sdk.createTextMessage(
  conversationId: 'conv123',
  senderId: 'user123',
  text: 'Hello, World!',
  tenantJson: '{}',
);

// 发送消息
final result = await sdk.sendMessage(jsonEncode(message));
print('消息发送成功: ${result['server_msg_id']}');
```

## 领域模型使用示例

### 创建会话实体

```dart
import 'package:flare_im/domain/entities/conversation.dart';
import 'package:flare_im/domain/value_objects/conversation_type.dart';

final conversation = Conversation(
  conversationId: 'conv123',
  conversationType: ConversationType.single,
  displayName: '张三',
  avatarUrl: 'https://example.com/avatar.jpg',
  unreadCount: 5,
  updatedAt: DateTime.now(),
  createdAt: DateTime.now(),
);

print('显示名称: ${conversation.displayTitle}');
print('有未读消息: ${conversation.hasUnread}');
```

### 创建消息实体

```dart
import 'package:flare_im/domain/entities/message.dart';
import 'package:flare_im/domain/value_objects/message_content.dart';

final message = Message(
  serverId: 'msg123',
  clientMsgId: 'client_msg_123',
  conversationId: 'conv123',
  senderId: 'user123',
  seq: 1,
  timestamp: DateTime.now(),
  clientTimestamp: DateTime.now(),
  content: const TextContent('Hello, World!'),
  status: MessageStatus.sent,
  source: MessageSource.local,
  senderName: '张三',
  senderAvatar: 'https://example.com/avatar.jpg',
  senderDisplayName: '张三',
);

print('消息预览: ${message.content.previewText}');
print('可以撤回: ${message.canRecall}');
```

## 开发指南

### 代码规范

- 使用 `snake_case` 命名文件和函数
- 使用 `PascalCase` 命名类型
- 使用 `SCREAMING_SNAKE_CASE` 命名常量
- 优先使用 `const` 构造函数
- 使用 `Equatable` 实现值相等

### DDD 分层原则

1. **领域层 (domain/)**: 纯业务逻辑,无外部依赖
2. **应用层 (application/)**: 编排业务逻辑,协调领域对象
3. **基础设施层 (infrastructure/)**: 具体实现,如 FFI、存储等
4. **接口层 (interface/)**: UI 组件和用户交互

### FFI 开发注意事项

1. 所有 C 函数调用必须通过 `SdkWrapper` 封装
2. 使用 `CallbackUtils` 处理异步回调
3. 注意内存管理,及时释放 native 内存
4. 使用 `nullptr` 表示空指针

## 测试

### 运行单元测试

```bash
flutter test test/unit/
```

### 运行集成测试

```bash
flutter test integration_test/
```

## 构建发布版本

### iOS

```bash
flutter build ios --release
```

### Android

```bash
flutter build apk --release
```

### macOS

```bash
flutter build macos --release
```

### Windows

```bash
flutter build windows --release
```

## 贡献指南

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add some amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件

## 联系方式

- 项目主页: https://github.com/flare-im/flare-im-flutter
- 问题反馈: https://github.com/flare-im/flare-im-flutter/issues

## FFI 契约版本

- Flutter 与 Rust `bindings/c` 使用严格事件协议，不做多协议兼容。
- 初始化阶段会调用 `flare_sdk_ffi_contract_version` 进行版本握手。
- 当前协议版本：`flare-im-ffi/v1`。
- 版本不一致时会在初始化阶段直接失败，避免线上出现“部分功能可用、部分事件丢失”的灰故障。
