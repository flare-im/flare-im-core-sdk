# Tauri 文件监听配置说明

## 问题
Tauri 在开发模式下会监听整个工作区目录（`flare-im-core-sdk`），导致 `examples/flare-im-12345/` 下的数据库文件变化会触发重新编译。

## 解决方案
已在以下位置创建 `.taurignore` 文件：

1. **工作区根目录** (`flare-im-core-sdk/.taurignore`)
   - 忽略 `examples/flare-im-*/` 目录
   - 忽略数据库文件

2. **项目根目录** (`examples/flare-chat/.taurignore`)
   - 忽略工作区中除当前项目外的所有内容
   - 忽略其他 examples 目录

## 如果仍然有问题
如果 `.taurignore` 文件不起作用，可以：

1. **删除旧的数据库目录**（推荐）：
   ```bash
   cd /Users/hg/workspace/flare/flare-im/flare-im-core-sdk
   ./cleanup_old_db.sh
   ```

2. **或者将数据库文件移到项目目录外**：
   使用环境变量 `FLARE_IM_DB_PATH` 指定数据库路径

## 验证
重启 `yarn tauri dev` 后，检查日志中是否还显示监听 `examples/flare-im-12345/` 目录。
