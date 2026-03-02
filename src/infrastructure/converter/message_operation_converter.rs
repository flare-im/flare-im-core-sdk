//! 消息操作转换器（基于 Event）
//!
//! 长连接协议中操作统一为 Event，本转换器负责 Event 与领域 MessageOperation 的双向转换。

use crate::domain::message::MessageOperation;
use crate::infrastructure::converter::error::ConversionError;
use crate::infrastructure::operation_event_builder::{operation_to_event, event_to_operation};
use flare_proto::common::Event;

/// 消息操作转换器（Domain MessageOperation ↔ Event）
pub struct MessageOperationConverter;

impl MessageOperationConverter {
    /// 从 Event 解析为领域 MessageOperation
    pub fn from_proto(event: &Event) -> Result<MessageOperation, ConversionError> {
        event_to_operation(event).map_err(|e| ConversionError::FieldMapping(e.to_string()))
    }

    /// 从领域 MessageOperation 转为 Event（conversation_id 为空时用 ""，仅用于测试往返）
    pub fn to_proto(domain: &MessageOperation) -> Result<Event, ConversionError> {
        operation_to_event(domain, "").map_err(|e| ConversionError::FieldMapping(e.to_string()))
    }
}
