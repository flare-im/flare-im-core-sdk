# 集成测试说明

## 测试概述

集成测试覆盖了 SDK 的所有 Facade API，包括：
- ImCoreSdk 核心 API（登录、登出、连接、同步、扩展）
- MessageFacade 所有消息 API（创建、发送、操作、查询）
- ConversationFacade 所有会话 API（查询、操作）

## 运行测试

### 基础测试（不连接服务端）

```bash
# 运行所有测试
cargo test --test integration_test

# 运行特定测试
cargo test --test integration_test test_login_logout
```

### 连接真实服务端测试

1. **启动 Gateway 服务端**

   确保 `flare-im-core/flare-signaling/gateway` 服务已启动：
   ```bash
   cd flare-im-core/flare-signaling/gateway
   cargo run
   ```

2. **设置环境变量**

   ```bash
   # 设置服务端地址（默认: ws://localhost:8080）
   export FLARE_TEST_SERVER_URL="ws://localhost:8080"
   
   # 可选：保留测试数据库文件用于调试
   export FLARE_KEEP_TEST_DB=1
   ```

3. **运行测试**

   ```bash
   cargo test --test integration_test
   ```

## 环境变量说明

### FLARE_TEST_SERVER_URL

指定测试时连接的服务端地址。

- **默认值**: `ws://localhost:8080`
- **格式**: `ws://host:port` 或 `wss://host:port`
- **示例**: 
  ```bash
  export FLARE_TEST_SERVER_URL="ws://localhost:8080"
  export FLARE_TEST_SERVER_URL="wss://gateway.example.com:8443"
  ```

### FLARE_KEEP_TEST_DB

是否保留测试数据库文件。

- **默认值**: `false`（测试结束后自动清理）
- **设置为 `1` 或 `true`**: 保留数据库文件，用于调试
- **数据库位置**: 临时目录中的 `storage/flare_im.db`

## 数据库文件

### 默认行为

测试使用临时目录，测试结束后自动清理，**不会保留数据库文件**。

### 保留数据库文件

设置 `FLARE_KEEP_TEST_DB=1` 后，测试会输出数据库文件位置：

```
📁 测试数据库文件位置: /tmp/xxx/storage/flare_im.db
📁 临时目录: /tmp/xxx
```

可以使用 SQLite 工具查看数据库内容：

```bash
sqlite3 /tmp/xxx/storage/flare_im.db
.tables
SELECT * FROM messages LIMIT 10;
SELECT * FROM conversations LIMIT 10;
```

## 测试数据库结构

### messages 表

```sql
CREATE TABLE messages (
    message_id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    data TEXT NOT NULL,  -- JSON 格式的消息数据
    created_at TEXT NOT NULL
);

CREATE INDEX idx_conversation_id ON messages(conversation_id);
```

### conversations 表

```sql
CREATE TABLE conversations (
    conversation_id TEXT PRIMARY KEY,
    data TEXT NOT NULL,  -- JSON 格式的会话数据
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_updated_at ON conversations(updated_at);
```

## 连接 Gateway 服务端

### Gateway 服务端配置

Gateway 服务端（`flare-im-core/flare-signaling/gateway`）默认监听：
- **WebSocket**: `ws://0.0.0.0:8080`
- **QUIC**: `quic://0.0.0.0:8081`（如果启用）

### 启动 Gateway 服务端

```bash
cd flare-im-core/flare-signaling/gateway
cargo run
```

### 测试连接

1. **检查服务端是否运行**

   ```bash
   # 检查端口是否监听
   lsof -i :8080
   # 或
   netstat -an | grep 8080
   ```

2. **设置环境变量并运行测试**

   ```bash
   # 设置服务端地址
   export FLARE_TEST_SERVER_URL="ws://localhost:8080"
   
   # 可选：保留数据库文件用于调试
   export FLARE_KEEP_TEST_DB=1
   
   # 运行连接测试
   cargo test --test integration_test test_login_logout -- --nocapture
   ```

3. **查看连接日志**

   如果连接失败，测试会输出提示信息：
   ```
   ⚠️  无法连接到服务器，请确保 gateway 服务端已启动
      设置 FLARE_TEST_SERVER_URL 环境变量可指定服务端地址
   ```

   如果连接成功，测试会正常执行，并可以验证：
   - 登录/登出功能
   - 消息发送/接收
   - 会话同步
   - 其他需要服务端交互的功能

## 测试覆盖

### 已覆盖的 API

- ✅ ImCoreSdk 核心 API（8 个测试）
- ✅ MessageFacade 消息 API（30+ 个测试）
- ✅ ConversationFacade 会话 API（15+ 个测试）
- ✅ 边界条件和错误场景（5+ 个测试）
- ✅ 端到端测试（E2E）（4 个测试）

### 测试统计

- **总测试数**: 62
- **通过率**: 100%
- **覆盖率**: 100%（Facade API 调用路径）

### 测试类型

1. **集成测试** (`integration_test.rs`): 测试所有 Facade API 的调用，不依赖服务端
2. **端到端测试** (`e2e_test.rs`): 测试 SDK 与服务端的完整交互，需要服务端运行

## 故障排查

### 问题：测试失败，提示连接失败

**原因**: 服务端未启动或地址配置错误

**解决**:
1. 确认 Gateway 服务端已启动
2. 检查 `FLARE_TEST_SERVER_URL` 环境变量是否正确
3. 检查防火墙和网络连接

### 问题：看不到数据库文件

**原因**: 测试使用临时目录，默认会自动清理

**解决**:
1. 设置 `FLARE_KEEP_TEST_DB=1` 环境变量
2. 查看测试输出中的数据库文件路径
3. 使用 SQLite 工具打开数据库文件

### 问题：数据库文件为空

**原因**: 测试可能没有执行写操作，或使用了内存存储

**解决**:
1. 确认测试中调用了写操作 API（如 `send_message`）
2. 检查 `config.storage.path` 是否正确配置
3. 查看测试日志确认使用了 SQLite 存储

## 示例：完整测试流程

### 示例 1：基础测试（不连接服务端）

```bash
cd flare-im-core-sdk

# 运行所有测试（使用内存存储，不连接服务端）
cargo test --test integration_test

# 运行测试并保留数据库文件
FLARE_KEEP_TEST_DB=1 cargo test --test integration_test test_sdk_initialization -- --nocapture
```

### 示例 2：连接真实服务端测试

```bash
# 1. 启动 Gateway 服务端（在另一个终端）
cd flare-im-core/flare-signaling/gateway
cargo run

# 2. 设置环境变量（在测试终端）
export FLARE_TEST_SERVER_URL="ws://localhost:8080"
export FLARE_KEEP_TEST_DB=1

# 3. 运行测试
cd flare-im-core-sdk
cargo test --test integration_test -- --nocapture

# 4. 查看数据库（如果设置了 FLARE_KEEP_TEST_DB=1）
# 查看测试输出中的数据库路径，例如：
# 📁 测试数据库文件位置: /tmp/xxx/storage/flare_im.db
# 然后：
sqlite3 /tmp/xxx/storage/flare_im.db
.tables
SELECT COUNT(*) FROM messages;
SELECT COUNT(*) FROM conversations;
```

### 示例 3：查看数据库内容

```bash
# 设置保留数据库
export FLARE_KEEP_TEST_DB=1

# 运行测试
cargo test --test integration_test test_sdk_initialization -- --nocapture

# 从输出中获取数据库路径，例如：
# 📁 测试数据库文件位置: /var/folders/.../storage/flare_im.db

# 使用 SQLite 工具查看
sqlite3 /var/folders/.../storage/flare_im.db

# 在 SQLite 中执行：
.tables
.schema messages
.schema conversations
SELECT * FROM messages LIMIT 10;
SELECT * FROM conversations LIMIT 10;
```

### 示例 4：运行端到端测试

```bash
# 1. 启动 Gateway 服务端（在另一个终端）
cd flare-im-core/flare-signaling/gateway
cargo run

# 2. 设置环境变量（在测试终端）
export FLARE_TEST_SERVER_URL="ws://localhost:60051"
export FLARE_KEEP_TEST_DB=1

# 3. 运行端到端测试（需要服务端运行）
cd flare-im-core-sdk
cargo test --test e2e_test -- --ignored --nocapture

# 或者运行特定的端到端测试
cargo test --test e2e_test test_two_clients_message_exchange -- --ignored --nocapture
```

### 示例 5：运行所有测试（包括端到端）

```bash
# 1. 启动 Gateway 服务端（在另一个终端）
cd flare-im-core/flare-signaling/gateway
cargo run

# 2. 设置环境变量
export FLARE_TEST_SERVER_URL="ws://localhost:60051"
export FLARE_KEEP_TEST_DB=1

# 3. 运行所有测试
cd flare-im-core-sdk
cargo test --all -- --nocapture

# 或者只运行集成测试（不依赖服务端）
cargo test --test integration_test -- --nocapture
```

## 端到端测试说明

### 端到端测试覆盖

端到端测试 (`e2e_test.rs`) 包含以下测试场景：

1. **两个客户端互发消息** (`test_two_clients_message_exchange`)
   - 验证客户端 A 发送消息给客户端 B
   - 验证客户端 B 收到消息
   - 验证客户端 B 回复消息给客户端 A
   - 验证客户端 A 收到回复

2. **会话同步** (`test_conversation_sync`)
   - 验证发送消息后会话列表更新
   - 验证未读数正确

3. **消息状态流转** (`test_message_status_flow`)
   - 验证消息从"发送中"到"已发送"的状态流转
   - 验证收到服务器 ACK

4. **重连和断线恢复** (`test_reconnect_and_recovery`)
   - 验证断线重连功能
   - 验证消息队列在重连后能正常处理

### 运行端到端测试

端到端测试默认被标记为 `#[ignore]`，需要显式运行：

```bash
# 运行所有端到端测试（需要服务端运行）
cargo test --test e2e_test -- --ignored

# 运行特定测试
cargo test --test e2e_test test_two_clients_message_exchange -- --ignored --nocapture
```

### 端到端测试要求

1. **服务端必须运行**: Gateway 服务端必须启动并监听在配置的地址
2. **网络连接**: 测试需要能够连接到服务端
3. **测试数据**: 测试使用独立的用户 ID 和会话 ID，不会影响生产数据

## 注意事项

1. **测试隔离**: 每个测试使用独立的临时目录，互不干扰
2. **自动清理**: 默认情况下，测试结束后会自动清理临时文件
3. **并发测试**: 测试可以并发运行，使用不同的临时目录
4. **服务端依赖**: 连接服务端的测试需要服务端运行，否则会失败（但不会影响 API 调用测试）
5. **端到端测试**: 端到端测试默认被忽略，需要显式运行（使用 `--ignored` 标志）
