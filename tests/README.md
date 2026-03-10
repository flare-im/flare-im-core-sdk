# Flare IM Core SDK 集成测试

## 运行方式

- **仅单元测试（无需服务端）**  
  `cargo test --test message_test`  
  `cargo test --test conversation_test`

- **集成测试（需本地或远程服务端）**  
  `cargo test --features integration-tests --test message_test --test conversation_test -- --ignored --nocapture`

## 前置条件

1. 已启动：gateway、flare-orchestrator、flare-storage-writer、Redis、Kafka、PostgreSQL、Consul 等（见 flare-im-core 部署文档）。
2. **修改过 orchestrator 或 storage-writer 代码后，必须重新编译并重启对应进程**，再跑集成测试，否则会连到旧逻辑导致用例失败。

## 覆盖范围

- **message_test**：发消息、撤回/编辑/删除、已读、正在输入、反应/置顶/标记、拉取会话消息、同步完整性（无漏消息）。
- **conversation_test**：发消息后会话列表/详情、标记已读、删除会话、同步会话。

## 通过标准

- 所有 `server_tests::*` 集成用例通过。
- SDK 会校验服务端返回的 `OperationResponse.status`（非 OK 会报错），确保操作在服务端真实成功。
- 同步完整性用例 `test_sync_no_missing_messages` 保证发送的 N 条消息在 list 中都能查到，无漏消息。

## 性能与规模（十亿级 / 毫秒延迟）

- 服务端已做：操作事件直发 Kafka（不经过缓冲）、Redis 序列号分配、独立 flush 计时器避免相互阻塞。
- 达到毫秒级延迟与十亿级规模需：Redis/Kafka 低延迟部署、按会话/租户分片、Storage Writer 批量写库与分区策略、网关与 orchestrator 水平扩展；详见项目 `doc/SCALABILITY.md` 与 `doc/PERFORMANCE_OPTIMIZATION.md`（若存在）。
