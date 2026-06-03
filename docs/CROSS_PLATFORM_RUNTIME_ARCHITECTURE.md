# Core SDK Cross-Platform Runtime Architecture

本文档只约束 `flare-im-core-sdk` 内部结构。目标是让消息、会话、同步等核心业务与平台无关；Web、React Native、uni-app、Android、iOS、Flutter 的媒体与存储差异统一通过 `platform/ports`、`platform/adapters`、`platform/runtime` 装配。

## Directory Layout

```text
src/
  domain/                 # 领域模型、仓储 trait、业务不变量；禁止平台 IO
    conversation/id.rs    # 与服务端一致的 CID 生成/校验规则
  application/            # 用例编排、同步任务、命令/查询；只依赖 ports/domain
    commands/             # 写命令
    queries/              # 读查询
    usecases/             # 消息/会话/同步用例编排
    projections/          # 本地读模型投影
    services/             # 去重、收敛、消息构建等应用服务
    lifecycle/            # 本地会话生命周期
    callbacks/            # 进度回调和宿主回调合同
  core/                   # 引擎、dispatcher、sync manager、可靠队列接入
  client/
    api/                  # 稳定 Facade：messages/conversations/media/capabilities
      presence/           # native/web 平台差异实现，统一导出 PresenceApi
    builder.rs            # IMClient 装配入口，消费 RuntimeComponents
    lifecycle.rs          # 登录配置、token、用户存储选择
    profile_center.rs     # 面向客户端 UI 的 profile center 合同
  platform/
    ports/
      media.rs            # MediaServicePort / MediaSourceDescriptor
      storage.rs          # StoreProvider 与仓储 trait 的稳定导出
      runtime.rs          # clock/spawner 等运行时端口
      transport.rs        # 未来 transport host adapter 边界
    adapters/
      media/              # 原生文件媒体实现、wasm stub、upload-only host wrapper
      storage/            # Memory/SQLite 打开逻辑，IndexedDB/custom host 注入规则
      mod.rs              # PlatformAdapterProfile 平台能力矩阵
    runtime/
      mod.rs              # PlatformKind / RuntimeConfig / RuntimeComponents
      native.rs           # 原生 runtime assembler
  infrastructure/         # SQLite/HTTP/socket/protocol 具体基础设施
  shared/                 # error/types/config/util 等跨层基础能力
```

## Boundary Rules

- `domain` 不知道 Web Blob、RN URI、SQLite、IndexedDB 或平台目录。
- `application` 可以识别稳定的 `MediaSourceDescriptor`，但不能直接读平台文件、Blob 或数据库。
- `client::builder` 是默认装配点：优先使用 `RuntimeComponents` 注入的 host adapters；没有注入时才创建原生默认适配器。
- `StoreProvider` 是存储唯一稳定入口。所有 Message/Conversation/PendingSend/Profile/MediaCache 仓储都挂在它下面。
- `MediaServicePort` 是媒体唯一稳定入口。上传、取链、缓存、用户下载目录管理都通过这个端口；不支持的 host 能力必须返回稳定的 `OperationNotSupported`。

## Platform Adapter Matrix

| Platform | Storage profile | Media source profile | Adapter rule |
| --- | --- | --- | --- |
| Web | IndexedDB, host injected | Blob, bytes/data URI | 必须注入 `StoreProvider` 与 Web media adapter |
| React Native | SQLite, host injected | file URI, asset, bytes | JS/native bridge 注入 `StoreProvider` 与 media adapter |
| uni-app | SQLite, host injected | temp file URI, asset, bytes | uni runtime 注入存储与媒体 adapter |
| Android | SQLite built-in or injected | file path, `content://`, asset | 默认可用 SQLite；`content://` 建议由 Android adapter 处理 |
| iOS | SQLite built-in or injected | file path, `ph://`, asset | 默认可用 SQLite；PhotoKit asset 由 iOS adapter 处理 |
| Flutter | SQLite built-in or injected | path, URI, asset | Flutter plugin 可注入 host media/storage |
| Tauri/Electron/Native | SQLite built-in | file path, `file://` | 默认使用原生文件媒体服务 |

`PlatformAdapterProfile::for_platform(platform)` 是上层生成绑定或初始化时的能力判断入口。

## Assembly Flow

```text
Platform wrapper
  -> choose PlatformKind
  -> read PlatformAdapterProfile
  -> build StoreProvider
  -> build MediaServicePort
  -> RuntimeComponents { stores, transport, media_service, ... }
  -> IMClient::builder().config(...).runtime(...).build()
```

未注入 `media_service` 时，builder 创建默认 `platform::adapters::media::MediaService`。只注入 `media_uploader` 时，builder 会用 `UploadOnlyMediaService` 包装它，上传可用，缓存/下载管理返回不支持。

## Media Send Flow

```text
MessageApi::sendWithMedia
  -> MessageSendUseCase
  -> extract_media_source(path/file/content/ph/blob/data/asset URI)
  -> MediaServicePort::upload(ProcessedMedia)
  -> UploadedMedia
  -> typed message media fields
  -> SendMessageCommand
```

稳定语义必须进入 typed fields，例如 `file_id`、`mime_type`、`size`、`image_id`。`metadata` 只用于扩展逃生口，不能承载核心媒体语义。

## Storage Flow

```text
RuntimeConfig.storage
  -> platform::adapters::storage::open_store_from_runtime_config
  -> StoreProvider
  -> SdkEngine / SyncProtocolAdapter / usecases
```

Web/RN/uni-app 的数据库实现由 host 注入 `StoreProvider`，core-sdk 不绑定具体 IndexedDB 或 JS SQLite 库。原生平台可使用 `lifecycle-sqlite` feature 打开 per-user SQLite。

## Extension Points

- 新平台优先实现 `StoreProvider` 与 `MediaServicePort`，不要修改 domain/application。
- 如果只需上传，先实现 `MediaUploaderPort` 并通过 `RuntimeComponents::with_media_uploader` 注入。
- 如果平台要支持缓存、取链、用户下载目录，直接实现完整 `MediaServicePort`。
- 能力缺口通过 `PlatformAdapterProfile` 和稳定错误显式暴露，不在业务层做平台分支。
