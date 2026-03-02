# IndexedDB 存储实现

IndexedDB 存储实现（Web 平台专用）。

## 状态

待实现。

## 设计

- `EventStore`: 基于 IndexedDB 的事件存储
- `MessageRepository`: 基于 IndexedDB 的消息仓储
- `ConversationRepository`: 基于 IndexedDB 的会话仓储
- `SnapshotStore`: 基于 IndexedDB 的快照存储（可选）

## 使用场景

适用于 Web 平台（wasm32），使用浏览器的 IndexedDB API 进行存储。
