# Flare IM Core 读/未读语义

本文定义 Core / Core SDK 的会话已读与未读统计契约。该设计保持 Core 业务无关，Social 或其他业务层只消费会话摘要、消息事件与已读命令，不在业务层重新计算核心未读。

## 架构方向

未读数采用“服务端摘要权威 + 客户端在线增量”的模型：

- 冷启动、重连、跨设备恢复时，`ConversationsSyncRes` 中的 `unread_count` / `last_read_seq` / `max_seq` 是会话列表的权威快照。
- 在线收到新消息时，SDK 只对“本地之前没见过的对端消息”做增量累加，不用 `max_seq - last_read_seq` 反推。
- 用户显式阅读会话时，SDK 本地推进 `last_read_seq` 并清零/修正未读，然后向服务端发送显式 read ack。
- 普通送达 ACK 不能推进已读位点，避免“收到消息即已读”。

## 字段语义

- `max_seq`：当前会话已知最大消息序列号，可来自服务端摘要或本地新消息投影。
- `last_read_seq`：当前用户确认已读到的最大序列号。
- `unread_count`：当前用户视角下未读消息数，只统计对端发来的、未撤回的、超过读位的消息。
- `peer_read_seq`：对端已读到的序列号，仅用于己方已发送消息的已读状态展示，不参与当前用户未读统计。

## 同步流程

### 冷启动 / 重连

1. SDK 先把本地真实已读位点通过 read ack 推给服务端，帮助服务端 participant 读位收敛。
2. SDK 拉取会话摘要并保存摘要。
3. 保存摘要后若 `unread_count` 变化，发布 `UnreadCountChanged`。
4. SDK 再按会话补拉消息。消息同步回放只补齐消息表和最后一条消息投影，不覆盖服务端摘要里的未读快照。

禁止在摘要保存后立即用本地消息表重算未读。冷启动时本地消息表可能还不完整，这会把服务端权威未读覆盖成 0 或错误的大数。

### 在线新消息

在线 push 进入 SDK 后，消息投影只计算增量：

- 必须属于当前会话。
- `sender_id != current_user_id`。
- `seq > last_read_seq`。
- 消息未撤回。
- `seq > previous.max_seq`，即本地之前没见过。

增量累加后发布 `UnreadCountChanged`。这样不会因为 seq 存在历史缺口而出现 `max_seq - last_read_seq` 造成的大未读，也不会在摘要已经推进时重复累加。

### 用户已读

业务层只能通过 SDK 的 `conversation.mark_read(conversation_id, read_seq)` 表达“用户看到了”。SDK 会：

1. 本地更新会话读位与未读数。
2. 更新消息已读状态。
3. 发送 typed `ReadAck { conversation_id, read_seq, device_id, ack_id }`。

服务端 route 层只接受 `AckPayload::Read` 写入 `ConversationManageService::MarkConversationAsRead`。`ConversationAck` 表示送达/会话处理位点，不推进 `last_read_seq`。

## 取舍

- 不用 `max_seq - last_read_seq` 直接作为未读数，是为了支持稀疏 seq、历史消息未完全落库、撤回消息、自发消息不计未读等真实 IM 场景。
- 冷启动相信服务端摘要，是为了保证多设备、离线期间和跨端已读后的最终一致。
- 在线用客户端增量，是为了低延迟更新会话列表，不等待下一轮摘要同步。
- read ack 使用独立 `ReadAck` payload，是为了避免稳定已读语义藏在 `metadata` 中，被普通送达 ACK 污染。

## 扩展建议

- 服务端为 read ack 增加幂等键与设备维度审计，便于排查多端读位回退。
- 会话摘要增加 `unread_version` 或 `participant_version`，客户端可用于冲突检测。
- 指标中增加：read ack apply count、delivery ack skipped count、unread delta count、summary unread overwrite count。

## 常见问题

- 重启后未读突然变多：通常是本地读位没有成功上报，或服务端 participant 读位落后。
- 重启后没读却显示无未读：通常是 delivery ack 被误当 read ack，服务端提前推进了 `last_read_seq`。
- 在线收到消息未读不增加：通常是消息投影没有发布 `UnreadCountChanged`，或本地把新消息误判为已见过。
- 未读出现很大的数字：通常是用 `max_seq - last_read_seq` 把历史 seq 缺口也算进了未读。

