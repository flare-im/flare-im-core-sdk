# 变更记录

本文件只记录**已发布到 crates.io 的版本**。版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。

## 1.2.0 — 未发布

### 新增

- 跟进契约层新增的两个会话参与者字段（需要 `flare-proto` 2.1.0）：
  `is_mention_only`（只接收提到我的消息）与 `visible_from_seq`（可见历史下限）。

### 修复

- 调度与恢复路径的失败改为记日志，不再静默吞掉。此前这类失败没有任何信号，
  只能靠用户报障才发现。
- `media.user_download_get_saved_path` 只返回仍在磁盘上的文件：用户删掉或挪走了保存的文件，
  它删掉这条下载记录并返回 `null`，客户端据此把「在文件夹中显示」换回「下载」。

### 依赖

- `flare-proto` / `flare-grpc-proto` → 2.1.0，`flare-core` → 1.1.1。
  后者含三批 RUSTSEC 修复，**建议一并升级**。

## 1.1.0 — 2026-08-03

与契约层 `flare-proto` 2.0.1 对齐的发布。
