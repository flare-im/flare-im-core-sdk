//! JSON ↔ Domain Model 转换器

use crate::domain::message::Message;
use crate::domain::conversation::Conversation;
use crate::infrastructure::converter::traits::{Converter, BatchConverter};
use crate::infrastructure::converter::error::ConversionError;
use serde_json::Value;

/// JSON 到领域模型转换器 - Message
pub struct MessageJsonConverter;

impl Converter<Value, Message> for MessageJsonConverter {
    fn convert(&self, json: Value) -> Result<Message, ConversionError> {
        serde_json::from_value(json)
            .map_err(|e| ConversionError::Deserialization(e.to_string()))
    }
    
    fn convert_back(&self, message: Message) -> Result<Value, ConversionError> {
        serde_json::to_value(&message)
            .map_err(|e| ConversionError::Serialization(e.to_string()))
    }
}

impl BatchConverter<Value, Message> for MessageJsonConverter {}

/// JSON 到领域模型转换器 - Conversation
pub struct ConversationJsonConverter;

impl Converter<Value, Conversation> for ConversationJsonConverter {
    fn convert(&self, json: Value) -> Result<Conversation, ConversionError> {
        serde_json::from_value(json)
            .map_err(|e| ConversionError::Deserialization(e.to_string()))
    }
    
    fn convert_back(&self, conversation: Conversation) -> Result<Value, ConversionError> {
        serde_json::to_value(&conversation)
            .map_err(|e| ConversionError::Serialization(e.to_string()))
    }
}

impl BatchConverter<Value, Conversation> for ConversationJsonConverter {}
