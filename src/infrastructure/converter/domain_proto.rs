//! Domain Model ↔ Protobuf 转换器

use crate::domain::message::Message;
use crate::infrastructure::converter::traits::{Converter, BatchConverter};
use crate::infrastructure::converter::error::ConversionError;
use crate::infrastructure::converter::MessageConverter as LegacyMessageConverter;
use flare_proto::flare::common::v1::Message as ProtoMessage;

/// 领域模型到 Protobuf 转换器 - Message
pub struct MessageProtoConverter;

impl Converter<Message, ProtoMessage> for MessageProtoConverter {
    fn convert(&self, message: Message) -> Result<ProtoMessage, ConversionError> {
        LegacyMessageConverter::to_proto(&message)
            .map_err(|e| ConversionError::FieldMapping(e.to_string()))
    }
    
    fn convert_back(&self, proto: ProtoMessage) -> Result<Message, ConversionError> {
        LegacyMessageConverter::from_proto(&proto)
            .map_err(|e| ConversionError::FieldMapping(e.to_string()))
    }
}

impl BatchConverter<Message, ProtoMessage> for MessageProtoConverter {}
