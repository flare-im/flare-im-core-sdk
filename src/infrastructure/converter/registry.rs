//! 转换器注册中心

use std::sync::Arc;
use crate::infrastructure::converter::traits::Converter;
use crate::infrastructure::converter::error::ConversionError;
use crate::infrastructure::converter::json_domain::{MessageJsonConverter, ConversationJsonConverter};
use crate::infrastructure::converter::domain_proto::MessageProtoConverter;
use crate::domain::message::Message;
use crate::domain::conversation::Conversation;
use serde_json::Value;
use flare_proto::flare::common::v1::Message as ProtoMessage;

/// 转换器注册中心
/// 
/// 统一管理所有转换器，支持动态注册和查找
pub struct ConverterRegistry {
    message_json_converter: Arc<MessageJsonConverter>,
    conversation_json_converter: Arc<ConversationJsonConverter>,
    message_proto_converter: Arc<MessageProtoConverter>,
}

impl ConverterRegistry {
    /// 创建新的转换器注册中心并注册所有默认转换器
    pub fn new() -> Self {
        Self {
            message_json_converter: Arc::new(MessageJsonConverter),
            conversation_json_converter: Arc::new(ConversationJsonConverter),
            message_proto_converter: Arc::new(MessageProtoConverter),
        }
    }
    
    /// JSON → Message
    pub fn json_to_message(&self, json: Value) -> Result<Message, ConversionError> {
        self.message_json_converter.convert(json)
    }
    
    /// Message → JSON
    pub fn message_to_json(&self, message: Message) -> Result<Value, ConversionError> {
        self.message_json_converter.convert_back(message)
    }
    
    /// JSON → Messages (批量)
    pub fn json_to_messages(&self, items: Vec<Value>) -> Result<Vec<Message>, ConversionError> {
        items
            .into_iter()
            .map(|json| self.json_to_message(json))
            .collect()
    }
    
    /// Messages → JSON (批量)
    pub fn messages_to_json(&self, messages: Vec<Message>) -> Result<Vec<Value>, ConversionError> {
        messages
            .into_iter()
            .map(|msg| self.message_to_json(msg))
            .collect()
    }
    
    /// JSON → Conversation
    pub fn json_to_conversation(&self, json: Value) -> Result<Conversation, ConversionError> {
        self.conversation_json_converter.convert(json)
    }
    
    /// Conversation → JSON
    pub fn conversation_to_json(&self, conversation: Conversation) -> Result<Value, ConversionError> {
        self.conversation_json_converter.convert_back(conversation)
    }
    
    /// JSON → Conversations (批量)
    pub fn json_to_conversations(&self, items: Vec<Value>) -> Result<Vec<Conversation>, ConversionError> {
        items
            .into_iter()
            .map(|json| self.json_to_conversation(json))
            .collect()
    }
    
    /// Conversations → JSON (批量)
    pub fn conversations_to_json(&self, conversations: Vec<Conversation>) -> Result<Vec<Value>, ConversionError> {
        conversations
            .into_iter()
            .map(|conv| self.conversation_to_json(conv))
            .collect()
    }
    
    /// Message → ProtoMessage
    pub fn message_to_proto(&self, message: Message) -> Result<ProtoMessage, ConversionError> {
        self.message_proto_converter.convert(message)
    }
    
    /// ProtoMessage → Message
    pub fn proto_to_message(&self, proto: ProtoMessage) -> Result<Message, ConversionError> {
        self.message_proto_converter.convert_back(proto)
    }
    
    /// Messages → ProtoMessages (批量)
    pub fn messages_to_proto(&self, messages: Vec<Message>) -> Result<Vec<ProtoMessage>, ConversionError> {
        messages
            .into_iter()
            .map(|msg| self.message_to_proto(msg))
            .collect()
    }
    
    /// ProtoMessages → Messages (批量)
    pub fn proto_to_messages(&self, protos: Vec<ProtoMessage>) -> Result<Vec<Message>, ConversionError> {
        protos
            .into_iter()
            .map(|proto| self.proto_to_message(proto))
            .collect()
    }
}

impl Default for ConverterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
