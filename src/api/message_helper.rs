//! 消息创建辅助函数
//!
//! 提供统一的 MessageBuilder 使用方式，简化消息创建

use crate::domain::{DomainMessage, MessageBuilder};
use anyhow::{Context, Result};

/// 从 MessageBuilder 构建 DomainMessage
pub fn build_domain_message(builder: MessageBuilder) -> Result<DomainMessage> {
    let proto_message = builder.build();
    DomainMessage::from_proto(proto_message).context("Failed to create message from proto")
}

/// 创建基础消息构建器
pub fn create_base_builder(session_id: &str, user_id: &str) -> MessageBuilder {
    MessageBuilder::new()
        .session_id(session_id.to_string())
        .sender_id(user_id.to_string())
}
