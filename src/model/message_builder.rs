//! Message 构建器 — 组合 content + 元数据，生成可发送的 Message。
//!
//! ```ignore
//! use flare_im_core_sdk::model::{ContentBuilder, MessageBuilder};
//!
//! // 文本消息
//! let msg = MessageBuilder::new("conv_123", "user_456")
//!     .content(ContentBuilder::text("Hello @All!").mention_all(6, 4).build())
//!     .build()?;
//!
//! // 图片消息（单聊）
//! let msg = MessageBuilder::new("conv_123", "user_456")
//!     .content(ContentBuilder::image("img_789").source(source_info).build())
//!     .receiver("user_789")
//!     .single_chat()
//!     .offline_push("新图片", "[图片]")
//!     .build()?;
//! ```

use std::collections::HashMap;

use flare_proto::common::{
    ConversationType, Message, MessageSource, OfflinePushInfo,
};

use crate::error::{SdkError, Result};
use crate::util::id::generate_client_msg_id;
use super::content_builder::BuiltContent;

pub struct MessageBuilder {
    conversation_id: String,
    sender_id: String,
    built_content: Option<BuiltContent>,
    receiver_id: String,
    channel_id: String,
    conversation_type: ConversationType,
    source: MessageSource,
    offline_push: Option<OfflinePushInfo>,
    extra: HashMap<String, String>,
    extensions: HashMap<String, Vec<u8>>,
}

impl MessageBuilder {
    pub fn new(conversation_id: impl Into<String>, sender_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            sender_id: sender_id.into(),
            built_content: None,
            receiver_id: String::new(),
            channel_id: String::new(),
            conversation_type: ConversationType::Unspecified,
            source: MessageSource::User,
            offline_push: None,
            extra: HashMap::new(),
            extensions: HashMap::new(),
        }
    }

    /// 设置消息内容（由 ContentBuilder 构建）
    pub fn content(mut self, content: BuiltContent) -> Self {
        self.built_content = Some(content);
        self
    }

    /// 设置接收者（单聊时使用）
    pub fn receiver(mut self, id: impl Into<String>) -> Self {
        self.receiver_id = id.into();
        self
    }

    /// 设置频道 ID（群聊/频道时使用）
    pub fn channel(mut self, id: impl Into<String>) -> Self {
        self.channel_id = id.into();
        self
    }

    /// 标记为单聊
    pub fn single_chat(mut self) -> Self {
        self.conversation_type = ConversationType::Single;
        self
    }

    /// 标记为群聊
    pub fn group_chat(mut self) -> Self {
        self.conversation_type = ConversationType::Group;
        self
    }

    /// 标记为频道
    pub fn channel_chat(mut self) -> Self {
        self.conversation_type = ConversationType::Channel;
        self
    }

    /// 设置会话类型
    pub fn conversation_type(mut self, ct: ConversationType) -> Self {
        self.conversation_type = ct;
        self
    }

    /// 设置消息来源
    pub fn source(mut self, source: MessageSource) -> Self {
        self.source = source;
        self
    }

    /// 设置离线推送信息
    pub fn offline_push(mut self, title: impl Into<String>, body: impl Into<String>) -> Self {
        self.offline_push = Some(OfflinePushInfo {
            title: title.into(),
            body: body.into(),
            ..Default::default()
        });
        self
    }

    /// 设置离线推送（完整配置）
    pub fn offline_push_info(mut self, info: OfflinePushInfo) -> Self {
        self.offline_push = Some(info);
        self
    }

    /// 添加扩展键值对
    pub fn extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }

    /// 添加业务扩展（二进制）
    pub fn extension(mut self, key: impl Into<String>, value: Vec<u8>) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    /// 构建 Message
    pub fn build(self) -> Result<Message> {
        let built = self.built_content
            .ok_or_else(|| SdkError::CommandFailed("content is required".into()))?;

        Ok(Message {
            client_msg_id: generate_client_msg_id(),
            conversation_id: self.conversation_id,
            sender_id: self.sender_id,
            receiver_id: self.receiver_id,
            channel_id: self.channel_id,
            conversation_type: self.conversation_type as i32,
            message_type: built.message_type as i32,
            content: built.encode(),
            source: self.source as i32,
            offline_push_info: self.offline_push,
            extra: self.extra,
            extensions: self.extensions,
            ..Default::default()
        })
    }
}

// ── 便捷快速构造 ────────────────────────────────────────

impl MessageBuilder {
    /// 快速创建文本消息
    pub fn text(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<Message> {
        Self::new(conversation_id, sender_id)
            .content(super::content_builder::ContentBuilder::text(text).build())
            .build()
    }

    /// 从 BuiltContent 快速创建消息
    pub fn quick(
        conversation_id: impl Into<String>,
        sender_id: impl Into<String>,
        content: BuiltContent,
    ) -> Result<Message> {
        Self::new(conversation_id, sender_id)
            .content(content)
            .build()
    }
}
