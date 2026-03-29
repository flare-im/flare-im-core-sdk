//! 下行载荷：与 flare-proto 对齐；DATA 同步/扩展经 `data.proto` `DataPacket` 解码为 `SyncRes` 或 `CustomData`。

use flare_proto::common::{CustomData, Event, EventEnvelope, MessagePush, SendAck, SyncRes};

/// 服务端下行包载荷（与 proto 类型一一对应）
#[derive(Clone)]
pub enum DownlinkPayload {
    /// 消息推送（message.proto MessagePush）
    MessagePush(MessagePush),
    /// 单条事件（event.proto Event）
    Event(Event),
    /// 事件批（event.proto EventEnvelope）
    EventEnvelope(EventEnvelope),
    /// 发消息回执（ack.proto Ack.payload.send）
    SendAck(SendAck),
    /// 自定义数据（data.proto CustomData）
    CustomData(CustomData),
    /// 单会话同步响应（sync.proto SyncRes）
    SyncResp(SyncRes),
}
