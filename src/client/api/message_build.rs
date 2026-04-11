//! 消息构建 Facade — 所有 `create_*` 必须传入 `conversation_id`；当前用户与 channel 由 SDK/会话内部解析，产出 [`crate::model::message::IMMessage`]。
//! 与 [`super::MessageApi::send`] 配合：先构建再发送。实现委托 [`crate::application::MessageBuilderService`]。
//!
//! 发送前根据本地 [`Conversation`] 记录直接写入消息的 `channel_id` 与 `conversation_type`（映射为 proto 枚举），供 Orchestrator 构建推送目标。

use std::sync::Arc;

use crate::application::MessageBuilderService;
use crate::conversation;
use crate::core::CurrentUserIdStore;
use crate::domain::ConversationStore;
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::BuiltContent;
use crate::model::message::IMMessage;
use flare_proto::common::ImageInfo;

/// 多类型消息的构建入口（不负责发送）。
pub struct MessageBuildApi {
    current_user_id: CurrentUserIdStore,
    conversations: Arc<dyn ConversationStore>,
}

impl MessageBuildApi {
    pub fn new(
        current_user_id: CurrentUserIdStore,
        conversations: Arc<dyn ConversationStore>,
    ) -> Self {
        Self {
            current_user_id,
            conversations,
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
            .conversations
            .get(conversation_id)
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
        let msg = MessageBuilderService::build_text(conversation_id, &sender_id, text, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_quote(
        &self,
        conversation_id: &str,
        quoted_message_id: &str,
        text: &str,
        quoted_sender_id: Option<&str>,
        quoted_text_preview: Option<&str>,
        quoted_content: Option<BuiltContent>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_quote(
            conversation_id,
            &sender_id,
            quoted_message_id,
            text,
            quoted_sender_id,
            quoted_text_preview,
            quoted_content,
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
            MessageBuilderService::build_thread_reply(conversation_id, &sender_id, thread_id, text)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_forward(
        &self,
        conversation_id: &str,
        merge: bool,
        title: &str,
        sources: Vec<IMMessage>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_forward(
            conversation_id,
            &sender_id,
            merge,
            title,
            &sources,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_with_content(
        &self,
        conversation_id: &str,
        content: BuiltContent,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg =
            MessageBuilderService::build_with_content(conversation_id, &sender_id, content, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_image(&self, conversation_id: &str, image_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_image(conversation_id, &sender_id, image_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_image_with_thumbnail(
        &self,
        conversation_id: &str,
        source_image_id: &str,
        thumbnail_image_id: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_image_with_thumbnail(
            conversation_id,
            &sender_id,
            source_image_id,
            thumbnail_image_id,
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_image_group(
        &self,
        conversation_id: &str,
        images: Vec<ImageInfo>,
        description: impl Into<String>,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_image_group(
            conversation_id,
            &sender_id,
            images,
            description,
            metadata,
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_video(&self, conversation_id: &str, video_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_video(conversation_id, &sender_id, video_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_audio(&self, conversation_id: &str, audio_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_audio(conversation_id, &sender_id, audio_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_file(&self, conversation_id: &str, file_id: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_file(conversation_id, &sender_id, file_id, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_location(
        &self,
        conversation_id: &str,
        longitude: f64,
        latitude: f64,
        address: impl Into<String>,
        title: impl Into<String>,
        zoom: Option<u8>,
        snapshot_url: Option<String>,
        snapshot_local_path: Option<String>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_location(
            conversation_id,
            &sender_id,
            longitude,
            latitude,
            address,
            title,
            zoom,
            snapshot_url,
            snapshot_local_path,
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_card(
        &self,
        conversation_id: &str,
        id: &str,
        card_type: Option<&str>,
        title: Option<&str>,
        subtitle: Option<&str>,
        avatar: Option<&str>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let ct = match card_type {
            Some(s) if !s.is_empty() => s,
            _ => "user",
        };
        let msg = MessageBuilderService::build_card(
            conversation_id,
            &sender_id,
            id,
            ct,
            title.unwrap_or(""),
            subtitle.unwrap_or(""),
            avatar.unwrap_or(""),
            None,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_sticker(
        &self,
        conversation_id: &str,
        sticker_id: &str,
        package_id: Option<&str>,
        url: Option<&str>,
        width: Option<i32>,
        height: Option<i32>,
        sticker_format: Option<&str>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_sticker_with(
            conversation_id,
            &sender_id,
            sticker_id,
            None,
            package_id,
            url,
            width.unwrap_or(0),
            height.unwrap_or(0),
            sticker_format,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_emoji(&self, conversation_id: &str, emoji: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_emoji(conversation_id, &sender_id, emoji, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_link_card(
        &self,
        conversation_id: &str,
        url: &str,
        title: Option<&str>,
        description: Option<&str>,
        thumbnail_url: Option<&str>,
        site_name: Option<&str>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_link_card(
            conversation_id,
            &sender_id,
            url,
            None,
            title,
            description,
            thumbnail_url,
            site_name,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_mini_program(
        &self,
        conversation_id: &str,
        app_id: &str,
        title: Option<&str>,
        page_path: Option<&str>,
        thumbnail_url: Option<&str>,
        extra: Option<std::collections::HashMap<String, String>>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_mini_program(
            conversation_id,
            &sender_id,
            app_id,
            None,
            title,
            page_path,
            thumbnail_url,
            extra,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_rich_doc(
        &self,
        conversation_id: &str,
        doc_json: &str,
        content_schema: &str,
        plain_text: &str,
        input_format: Option<&str>,
        input_format_version: Option<i32>,
        source_payload: Option<std::collections::HashMap<String, String>>,
        title: Option<&str>,
        search_text: Option<&str>,
        render_hints_json: Option<&str>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_rich_doc(
            conversation_id,
            &sender_id,
            doc_json,
            content_schema,
            plain_text,
            None,
            input_format,
            input_format_version,
            source_payload,
            title,
            search_text,
            render_hints_json,
        )?;
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
            MessageBuilderService::build_system(conversation_id, &sender_id, event_kind, body)?;
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
            MessageBuilderService::build_notification(conversation_id, &sender_id, title, body)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_vote(
        &self,
        conversation_id: &str,
        vote_id: &str,
        title: &str,
        options: Vec<String>,
        participant_user_ids: Option<Vec<String>>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_vote(
            conversation_id,
            &sender_id,
            vote_id,
            title,
            options,
            None,
            participant_user_ids,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_task(
        &self,
        conversation_id: &str,
        task_id: &str,
        title: &str,
        status: Option<&str>,
        participant_user_ids: Option<Vec<String>>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_task(
            conversation_id,
            &sender_id,
            task_id,
            title,
            None,
            status,
            participant_user_ids,
        )?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_schedule(
        &self,
        conversation_id: &str,
        schedule_id: &str,
        title: &str,
        start_time_ms: i64,
        end_time_ms: i64,
        participant_user_ids: Option<Vec<String>>,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_schedule(
            conversation_id,
            &sender_id,
            schedule_id,
            title,
            None,
            start_time_ms,
            end_time_ms,
            participant_user_ids,
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
            MessageBuilderService::build_announcement(conversation_id, &sender_id, title, body)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_custom(&self, conversation_id: &str, r#type: &str) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_custom(conversation_id, &sender_id, r#type, None)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_placeholder(
        &self,
        conversation_id: &str,
        reason: &str,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_placeholder(conversation_id, &sender_id, reason)?;
        self.apply_conversation_routing(conversation_id, msg).await
    }
}
