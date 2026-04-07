# 媒体上传进度事件协议（统一规范）

本文档定义 SDK 与各平台绑定层共享的媒体上传进度协议，统一用于桌面端、移动端、Web 容器的上传进度展示与状态编排。

## 目标

- 统一上传进度事件语义，避免各端自行约定导致行为不一致
- 明确状态机与字段含义，支持稳定的 UI 进度展示
- 兼容单文件上传与分片上传（含断点续传）

## 适用范围

- SDK 核心：`flare-im-core-sdk`
- 绑定层：`bindings/tauri`（已接入），后续可平移到 UniFFI/C/移动端桥接
- 前端：通过 `im://upload_progress` 事件消费

## 触发入口

当前标准入口（Tauri）：

- 命令：`sdk_send_with_media_progress`
- 行为：发送消息前如果检测到本地媒体路径，SDK 先上传媒体并在上传过程中持续推送进度事件，然后继续发送消息

SDK 对应 API：

- `MessageApi::send_with_media_progress(...)`
- `MediaApi::upload_file_from_path_with_progress(...)`

## 事件名

- `im://upload_progress`

## Payload 结构（camelCase）

```json
{
  "fileName": "photo.png",
  "uploadId": "mp-1710000000000-1234",
  "phase": "Uploading",
  "uploadedBytes": 524288,
  "totalBytes": 2097152,
  "chunkIndex": 2,
  "totalChunks": 8
}
```

字段定义：

- `fileName`：展示用文件名
- `uploadId`：本次上传任务唯一 ID（一次上传链路内不变）
- `phase`：上传阶段，见状态机
- `uploadedBytes`：累计已上传字节（单调不减）
- `totalBytes`：总字节数
- `chunkIndex`：当前分片索引（0-based，可为空）
- `totalChunks`：总分片数（可为空）

## 上传阶段状态机

`phase` 允许值：

- `Preparing`
- `Uploading`
- `Completing`
- `Finished`

状态流转：

`Preparing` -> `Uploading` -> `Completing` -> `Finished`

说明：

- 单文件上传也会走同样阶段，但 `chunkIndex/totalChunks` 通常为 `0/1`
- 分片上传时 `Uploading` 可能出现多次，`uploadedBytes` 应持续增长
- 断点续传场景下，已完成分片会被计入 `uploadedBytes`，确保进度连续

## 前端消费规范（建议）

- 进度百分比计算：`percent = floor(uploadedBytes / totalBytes * 100)`
- 展示策略：
  - `Preparing`：显示“正在准备”
  - `Uploading`：显示“正在上传 x%”
  - `Completing`：显示“正在完成”
  - `Finished`：可短暂显示“上传完成”后切回发送态
- 去重键建议：`uploadId`（同一次上传生命周期内唯一）

## 错误与重试

- 上传失败不通过 `im://upload_progress` 传递错误；失败由命令返回错误（或上层 send failed）体现
- UI 重试建议复用同一路径重新触发命令
- 重试会产生新的 `uploadId`，应按新任务处理

## 跨端兼容要求

其他绑定层（UniFFI/C/移动端）接入时，必须保持：

- 事件名一致：`im://upload_progress`（或平台等价事件通道）
- 字段名一致：camelCase
- `phase` 字符串值一致
- `uploadedBytes` 单调不减，`Finished` 时应等于 `totalBytes`

## 版本演进

当前版本：`v1`

向后兼容约定：

- 新增字段只能“可选新增”，不得破坏现有字段语义
- `phase` 新增枚举值需先升级本文档并同步绑定层说明
