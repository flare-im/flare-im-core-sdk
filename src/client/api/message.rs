//! 消息 Facade — 直接编排消息写侧/读侧 usecase；发送前用 [`super::MessageBuildApi`] 构建 [`crate::model::message::IMMessage`] 再调用 [`MessageApi::send`]。
//!
//! **上层约定**：所有对上层暴露的 `message_id` 均为 **client_msg_id**；server_msg_id 仅用于内部与服务端交互。

use std::sync::Arc;

use super::media::UploadProgressCallback;
use crate::application::usecases::{
    MessageMutationUseCase, MessageSendUseCase, MessageViewAssembler,
};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::{BuiltContent, ContentBuilder};
use crate::model::message::{IMMessage, MarkType, SendAck};

/// 消息命令与查询入口（直接委托 application usecases）。
#[derive(Clone)]
pub struct MessageApi {
    send_use_case: Arc<MessageSendUseCase>,
    mutation_use_case: Arc<MessageMutationUseCase>,
    view_assembler: Arc<MessageViewAssembler>,
}

impl MessageApi {
    pub fn new(
        send_use_case: Arc<MessageSendUseCase>,
        mutation_use_case: Arc<MessageMutationUseCase>,
        view_assembler: Arc<MessageViewAssembler>,
    ) -> Self {
        Self {
            send_use_case,
            mutation_use_case,
            view_assembler,
        }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        self.send_use_case.current_user_id().await
    }

    /// 默认发送：若检测到本地媒体路径则先上传 OSS，再发送消息。
    pub async fn send(&self, message: IMMessage) -> Result<SendAck> {
        self.send_use_case.send_with_media(message, None).await
    }

    /// 带上传进度回调的发送：检测到本地媒体路径时，回调会上报上传阶段与字节进度。
    pub async fn send_with_media_progress(
        &self,
        message: IMMessage,
        on_progress: Option<UploadProgressCallback>,
    ) -> Result<SendAck> {
        self.send_use_case
            .send_with_media(message, on_progress)
            .await
    }

    /// 原样发送：不做 OSS 上传预处理。
    pub async fn send_no_oss(&self, message: IMMessage) -> Result<SendAck> {
        self.send_use_case.send(message).await
    }

    /// 撤回消息。message_id 为 client_msg_id。
    pub async fn recall(&self, message_id: &str) -> Result<()> {
        self.mutation_use_case.recall(message_id).await
    }

    /// 编辑消息内容。message_id 为 client_msg_id。
    pub async fn edit(
        &self,
        conversation_id: &str,
        message_id: &str,
        new_content: Vec<u8>,
    ) -> Result<()> {
        self.mutation_use_case
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
        self.mutation_use_case
            .edit(conversation_id, message_id, content.encode())
            .await
    }

    /// 按 message_id（client_msg_id）编辑为纯文本。
    pub async fn edit_text_by_message_id(&self, message_id: &str, text: &str) -> Result<()> {
        let (conversation_id, _) = self
            .mutation_use_case
            .resolve_message_id(message_id)
            .await?;
        self.edit_content(
            &conversation_id,
            message_id,
            crate::model::ContentBuilder::text(text).build(),
        )
        .await
    }

    /// 按 message_id 编辑为 Rich Doc（与 [`ContentBuilder::try_rich_doc`] 规则一致）。
    pub async fn edit_rich_doc_by_message_id(
        &self,
        message_id: &str,
        doc_json: impl Into<String>,
        content_schema: impl Into<String>,
        plain_text: impl Into<String>,
        input_format: Option<&str>,
        input_format_version: Option<i32>,
        source_payload: Option<std::collections::HashMap<String, String>>,
        title: Option<&str>,
        search_text: Option<&str>,
        render_hints_json: Option<&str>,
    ) -> Result<()> {
        let (conversation_id, _) = self
            .mutation_use_case
            .resolve_message_id(message_id)
            .await?;
        let mut cb =
            ContentBuilder::try_rich_doc(doc_json, content_schema, plain_text).map_err(|e| {
                FlareError::localized(
                    ErrorCode::InvalidParameter,
                    format!("sdk.message.rich_doc_v2.invalid: {e}"),
                )
            })?;
        if let Some(f) = input_format {
            cb = cb.rich_text_input_format(f);
        }
        if let Some(v) = input_format_version {
            cb = cb.rich_text_input_format_version(v);
        }
        if let Some(map) = source_payload {
            for (k, v) in map {
                cb = cb.rich_text_source_payload_entry(k, v);
            }
        }
        cb = cb.rich_text_title(title.map(|s| s.to_string()));
        cb = cb.rich_text_search_text(search_text.map(|s| s.to_string()));
        cb = cb.rich_text_render_hints_json(render_hints_json.map(|s| s.to_string()));
        let built = cb.build();
        self.edit_content(&conversation_id, message_id, built).await
    }

    /// 删除消息。message_id 为 client_msg_id。
    pub async fn delete(&self, message_id: &str) -> Result<()> {
        self.mutation_use_case
            .delete_for_self(message_id, None)
            .await
    }

    /// 仅删除自己可见（多端同步）。
    pub async fn delete_for_self(&self, message_id: &str, reason: Option<String>) -> Result<()> {
        self.mutation_use_case
            .delete_for_self(message_id, reason)
            .await
    }

    /// 删除所有人可见（仅发送者）。
    pub async fn delete_for_everyone(
        &self,
        message_id: &str,
        reason: Option<String>,
    ) -> Result<()> {
        self.mutation_use_case
            .delete_for_everyone(message_id, reason)
            .await
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.mutation_use_case
            .mark_read_with_ids(conversation_id, Vec::new(), read_seq)
            .await
    }

    pub async fn mark_read_with_ids(
        &self,
        conversation_id: &str,
        message_ids: Vec<String>,
        read_seq: u64,
    ) -> Result<()> {
        self.mutation_use_case
            .mark_read_with_ids(conversation_id, message_ids, read_seq)
            .await
    }

    pub async fn typing(&self, conversation_id: &str, typing: bool) -> Result<()> {
        self.mutation_use_case.typing(conversation_id, typing).await
    }

    /// 按 message_id（client_msg_id）添加反应。
    pub async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        self.mutation_use_case.add_reaction(message_id, emoji).await
    }

    /// 按 message_id（client_msg_id）移除反应。
    pub async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        self.mutation_use_case
            .remove_reaction(message_id, emoji)
            .await
    }

    /// 置顶消息。message_id 为 client_msg_id。
    pub async fn pin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        self.mutation_use_case
            .pin(conversation_id, message_id)
            .await
    }

    /// 取消置顶。message_id 为 client_msg_id。
    pub async fn unpin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        self.mutation_use_case
            .unpin(conversation_id, message_id)
            .await
    }

    /// 按 message_id（client_msg_id）置顶。
    pub async fn pin_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conversation_id, _) = self
            .mutation_use_case
            .resolve_message_id(message_id)
            .await?;
        self.pin(&conversation_id, message_id).await
    }

    /// 按 message_id（client_msg_id）取消置顶。
    pub async fn unpin_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conversation_id, _) = self
            .mutation_use_case
            .resolve_message_id(message_id)
            .await?;
        self.unpin(&conversation_id, message_id).await
    }

    /// 标记消息。message_id 为 client_msg_id。
    pub async fn mark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        self.mutation_use_case
            .mark(conversation_id, message_id, mark_type, "")
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
        self.mutation_use_case
            .mark(conversation_id, message_id, mark_type, color)
            .await
    }

    /// 取消标记。message_id 为 client_msg_id。
    pub async fn unmark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        self.mutation_use_case
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
        let (conversation_id, _) = self
            .mutation_use_case
            .resolve_message_id(message_id)
            .await?;
        self.mutation_use_case
            .mark(&conversation_id, message_id, mark_type, color)
            .await
    }

    /// 按 message_id（client_msg_id）取消标记。
    pub async fn unmark_by_message_id(&self, message_id: &str, mark_type: MarkType) -> Result<()> {
        let (conversation_id, _) = self
            .mutation_use_case
            .resolve_message_id(message_id)
            .await?;
        self.mutation_use_case
            .unmark(&conversation_id, message_id, mark_type)
            .await
    }

    /// 按 message_id（client_msg_id）查询单条消息。
    pub async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        self.view_assembler.get(message_id).await
    }

    /// 按 message_id（client_msg_id）查询原始消息（不填充发送者资料）。
    pub async fn get_raw(&self, message_id: &str) -> Result<Option<IMMessage>> {
        self.view_assembler.get_raw(message_id).await
    }

    /// `before_seq == 0`：会话首屏（本地最新一页）；否则 `seq < before_seq` 分页更早消息。
    pub async fn list(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        self.view_assembler
            .list(conversation_id, before_seq, limit)
            .await
    }

    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        self.view_assembler.search(keyword, limit).await
    }
}
