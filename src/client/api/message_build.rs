//! 消息构建 Facade — 所有 `create_*` 必须传入 `conversation_id`；当前用户与 channel 由 SDK/会话内部解析，产出 [`crate::model::message::IMMessage`]。
//! 与 [`super::MessageApi::send`] 配合：先构建再发送。实现委托 [`crate::application::handlers::MessageBuilderHandler`]。
//!
//! 发送前根据本地 [`Conversation`] 记录直接写入消息的 `channel_id` 与 `conversation_type`（映射为 proto 枚举），供 Orchestrator 构建推送目标。

use std::sync::Arc;

use crate::application::handlers::{ConversationQueryHandler, MessageBuilderHandler};
use crate::application::queries::GetConversationQuery;
use crate::conversation;
use crate::core::CurrentUserIdStore;
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::BuiltContent;
use crate::model::message::IMMessage;

/// 多类型消息的构建入口（不负责发送）。
pub struct MessageBuildApi {
    current_user_id: CurrentUserIdStore,
    conversation_query: Arc<ConversationQueryHandler>,
}

impl MessageBuildApi {
    pub fn new(
        current_user_id: CurrentUserIdStore,
        conversation_query: Arc<ConversationQueryHandler>,
    ) -> Self {
        Self {
            current_user_id,
            conversation_query,
        }
    }

    async fn current_sender_id(&self) -> Result<String> {
        let current_user_id = self.current_user_id.read().await.clone();
        if current_user_id.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(current_user_id)
    }

    /// 用本地会话行的 `channel_id`、`conversation_type` 覆盖消息对应字段（SDK 类型映射为 proto）。
    async fn apply_conversation_routing(
        &self,
        conversation_id: &str,
        mut msg: IMMessage,
    ) -> Result<IMMessage> {
        let Some(conv) = self
            .conversation_query
            .handle_get_conversation(GetConversationQuery {
                conversation_id: conversation_id.to_string(),
            })
            .await?
        else {
            if conversation::is_single_chat_conversation(conversation_id) {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "单聊会话未在本地落库，请先调用 conversation.get_one(对端 user_id, Single)",
                ));
            }
            return Ok(msg);
        };

        msg.channel_id = conv.channel_id.clone();
        msg.conversation_type = conv.conversation_type.to_proto_int();

        if conversation::is_single_chat_conversation(conversation_id) && msg.channel_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "单聊缺少对方 user_id（channel_id）。请先调用 conversation.get_one(对端, Single) 以补全会话行",
            ));
        }

        Ok(msg)
    }

    pub async fn create_text(&self, conversation_id: &str, text: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_text(conversation_id, &sender_id, text, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_quote(
        &self,
        conversation_id: &str,
        quoted_message_id: &str,
        text: &str,
        quoted_text_preview: Option<&str>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_quote(
            conversation_id,
            &sender_id,
            quoted_message_id,
            text,
            quoted_text_preview,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_thread_reply(
        &self,
        conversation_id: &str,
        thread_id: &str,
        text: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_thread_reply(conversation_id, &sender_id, thread_id, text)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_forward(
        &self,
        conversation_id: &str,
        message_ids: Vec<String>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_forward(conversation_id, &sender_id, message_ids)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_with_content(
        &self,
        conversation_id: &str,
        content: BuiltContent,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_with_content(conversation_id, &sender_id, content, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_image(&self, conversation_id: &str, image_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_image(conversation_id, &sender_id, image_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_video(&self, conversation_id: &str, video_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_video(conversation_id, &sender_id, video_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_audio(&self, conversation_id: &str, audio_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_audio(conversation_id, &sender_id, audio_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_file(&self, conversation_id: &str, file_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_file(conversation_id, &sender_id, file_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_location(
        &self,
        conversation_id: &str,
        longitude: f64,
        latitude: f64,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_location(
            conversation_id,
            &sender_id,
            longitude,
            latitude,
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_card(&self, conversation_id: &str, user_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_card(conversation_id, &sender_id, user_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_sticker(
        &self,
        conversation_id: &str,
        sticker_id: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_sticker(conversation_id, &sender_id, sticker_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_emoji(&self, conversation_id: &str, emoji: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_emoji(conversation_id, &sender_id, emoji, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_gif(&self, conversation_id: &str, gif_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_gif(conversation_id, &sender_id, gif_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_link_card(&self, conversation_id: &str, url: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_link_card(conversation_id, &sender_id, url, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_mini_program(
        &self,
        conversation_id: &str,
        app_id: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_mini_program(conversation_id, &sender_id, app_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_rich_text(
        &self,
        conversation_id: &str,
        body: &str,
        format: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_rich_text(conversation_id, &sender_id, body, format, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_markdown(&self, conversation_id: &str, text: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_markdown(conversation_id, &sender_id, text, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_system(
        &self,
        conversation_id: &str,
        event_kind: &str,
        body: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_system(conversation_id, &sender_id, event_kind, body)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_notification(
        &self,
        conversation_id: &str,
        title: &str,
        body: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_notification(conversation_id, &sender_id, title, body)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_vote(
        &self,
        conversation_id: &str,
        vote_id: &str,
        title: &str,
        options: Vec<String>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_vote(
            conversation_id,
            &sender_id,
            vote_id,
            title,
            options,
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_task(
        &self,
        conversation_id: &str,
        task_id: &str,
        title: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_task(conversation_id, &sender_id, task_id, title, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_schedule(
        &self,
        conversation_id: &str,
        schedule_id: &str,
        title: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_schedule(
            conversation_id,
            &sender_id,
            schedule_id,
            title,
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_announcement(
        &self,
        conversation_id: &str,
        title: &str,
        body: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderHandler::build_announcement(conversation_id, &sender_id, title, body)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_custom(&self, conversation_id: &str, r#type: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_custom(conversation_id, &sender_id, r#type, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_placeholder(
        &self,
        conversation_id: &str,
        reason: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderHandler::build_placeholder(conversation_id, &sender_id, reason)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }
}
