//! 消息 Facade — 委托 [`crate::application::MessageEngine`]；发送前用 [`super::MessageBuildApi`] 构建 [`crate::model::message::IMMessage`] 再调用 [`MessageApi::send`]。
//!
//! **上层约定**：所有对上层暴露的 `message_id` 均为 **client_msg_id**；server_msg_id 仅用于内部与服务端交互。

use std::sync::Arc;

use crate::application::MessageEngine;
use crate::error::Result;
use crate::model::content_builder::BuiltContent;
use crate::model::message::{IMMessage, MarkType, SendAck};

/// 消息命令与查询入口（逻辑在 `MessageEngine`）。
#[derive(Clone)]
pub struct MessageApi {
    engine: Arc<MessageEngine>,
}

impl MessageApi {
    pub fn new(engine: Arc<MessageEngine>) -> Self {
        Self { engine }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        self.engine.current_user_id().await
    }

    /// 统一发送：传入已构建的 IMMessage，内部流转。消息需通过 [`super::MessageBuildApi`] 构建（必须带会话 id）。
    pub async fn send(&self, message: IMMessage) -> Result<SendAck> {
        self.engine.send_message(message).await
    }

    /// 撤回消息。message_id 为 client_msg_id。
    pub async fn recall(&self, message_id: &str) -> Result<()> {
        self.engine.recall(message_id).await
    }

    /// 编辑消息内容。message_id 为 client_msg_id。
    pub async fn edit(
        &self,
        conversation_id: &str,
        message_id: &str,
        new_content: Vec<u8>,
    ) -> Result<()> {
        self.engine
            .edit(conversation_id, message_id, new_content)
            .await
    }

    /// 编辑消息（已构建内容）。message_id 为 client_msg_id。
    pub async fn edit_content(
        &self,
        conversation_id: &str,
        message_id: &str,
        content: BuiltContent,
    ) -> Result<()> {
        self.engine
            .edit_content(conversation_id, message_id, content)
            .await
    }

    /// 按 message_id（client_msg_id）编辑为纯文本。
    pub async fn edit_text_by_message_id(&self, message_id: &str, text: &str) -> Result<()> {
        self.engine.edit_text(message_id, text).await
    }

    /// 删除消息。message_id 为 client_msg_id。
    pub async fn delete(&self, message_id: &str) -> Result<()> {
        self.engine.delete(message_id).await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.engine.mark_read(conversation_id, read_seq).await
    }

    pub async fn mark_read_with_ids(
        &self,
        conversation_id: &str,
        message_ids: Vec<String>,
        read_seq: u64,
    ) -> Result<()> {
        self.engine
            .mark_read_with_ids(conversation_id, message_ids, read_seq)
            .await
    }

    pub async fn typing(&self, conversation_id: &str, typing: bool) -> Result<()> {
        self.engine.typing(conversation_id, typing).await
    }

    /// 按 message_id（client_msg_id）添加反应。
    pub async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        self.engine.add_reaction(message_id, emoji).await
    }

    /// 按 message_id（client_msg_id）移除反应。
    pub async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        self.engine.remove_reaction(message_id, emoji).await
    }

    /// 置顶消息。message_id 为 client_msg_id。
    pub async fn pin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        self.engine.pin(conversation_id, message_id).await
    }

    /// 取消置顶。message_id 为 client_msg_id。
    pub async fn unpin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        self.engine.unpin(conversation_id, message_id).await
    }

    /// 按 message_id（client_msg_id）置顶。
    pub async fn pin_by_message_id(&self, message_id: &str) -> Result<()> {
        self.engine.pin_by_message_id(message_id).await
    }

    /// 按 message_id（client_msg_id）取消置顶。
    pub async fn unpin_by_message_id(&self, message_id: &str) -> Result<()> {
        self.engine.unpin_by_message_id(message_id).await
    }

    /// 标记消息。message_id 为 client_msg_id。
    pub async fn mark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        self.engine
            .mark(conversation_id, message_id, mark_type)
            .await
    }

    /// 带颜色的标记。message_id 为 client_msg_id。
    pub async fn mark_with_color(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        self.engine
            .mark_with_color(conversation_id, message_id, mark_type, color)
            .await
    }

    /// 取消标记。message_id 为 client_msg_id。
    pub async fn unmark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        self.engine
            .unmark(conversation_id, message_id, mark_type)
            .await
    }

    /// 按 message_id（client_msg_id）标记。
    pub async fn mark_by_message_id(
        &self,
        message_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        self.engine
            .mark_by_message_id(message_id, mark_type, color)
            .await
    }

    /// 按 message_id（client_msg_id）取消标记。
    pub async fn unmark_by_message_id(&self, message_id: &str, mark_type: MarkType) -> Result<()> {
        self.engine
            .unmark_by_message_id(message_id, mark_type)
            .await
    }

    /// 按 message_id（client_msg_id）查询单条消息。
    pub async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        self.engine.get(message_id).await
    }

    /// 按 message_id（client_msg_id）查询原始消息（不填充发送者资料）。
    pub async fn get_raw(&self, message_id: &str) -> Result<Option<IMMessage>> {
        self.engine.get_raw(message_id).await
    }

    pub async fn list(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.engine.list(conversation_id, before_seq, limit).await
    }

    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.engine.search(keyword, limit).await
    }
}
