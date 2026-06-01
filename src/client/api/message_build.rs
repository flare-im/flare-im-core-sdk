//! 消息构建 Facade — 所有 `create_*` 必须传入 `conversation_id`；当前用户与 channel 由 SDK/会话内部解析，产出 [`crate::model::message::IMMessage`]。
//! 与 [`super::MessageApi::send`] 配合：先构建再发送。实现委托 [`crate::application::MessageBuilderService`]。
//!
//! 发送前根据本地 [`Conversation`] 记录直接写入消息的 `channel_id` 与 `conversation_type`（映射为 proto 枚举），供 Orchestrator 构建推送目标。

use std::sync::Arc;

use crate::application::{
    BuildCardRequest, BuildLinkCardRequest, BuildLocationRequest, BuildMiniProgramRequest,
    BuildRichDocRequest, BuildScheduleRequest, BuildStickerRequest, MessageBuilderService,
};
use crate::conversation;
use crate::core::CurrentUserIdStore;
use crate::domain::{ConversationIdentityService, ConversationStore};
use crate::error::{ErrorCode, FlareError, Result};
use crate::model::content_builder::BuiltContent;
use crate::model::message::IMMessage;
use flare_proto::common::ImageInfo;

#[derive(Clone, Debug)]
pub struct CreateLocationRequest {
    pub conversation_id: String,
    pub longitude: f64,
    pub latitude: f64,
    pub address: String,
    pub title: String,
    pub zoom: Option<u8>,
    pub snapshot_url: Option<String>,
    pub snapshot_local_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CreateStickerRequest {
    pub conversation_id: String,
    pub sticker_id: String,
    pub package_id: Option<String>,
    pub url: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub sticker_format: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CreateRichDocRequest {
    pub conversation_id: String,
    pub doc_json: String,
    pub content_schema: String,
    pub plain_text: String,
    pub input_format: Option<String>,
    pub input_format_version: Option<i32>,
    pub source_payload: Option<std::collections::HashMap<String, String>>,
    pub title: Option<String>,
    pub search_text: Option<String>,
    pub render_hints_json: Option<String>,
}

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
        let Some(mut conv) = self.conversations.get(conversation_id).await? else {
            if conversation::is_single_chat_conversation(conversation_id) {
                return Err(FlareError::localized(
                    ErrorCode::InvalidParameter,
                    "单聊会话未在本地落库，请先调用 conversation.get_one(对端 user_id, Single)",
                ));
            }
            return Ok(msg);
        };

        if conv.conversation_type.is_single_chat_conversation() {
            let current_user_id = self.current_sender_id().await?;
            let peer_hint = conv
                .participants
                .iter()
                .chain(conv.member_preview.iter())
                .map(|p| p.user_id.trim())
                .find(|id| !id.is_empty() && *id != current_user_id.as_str())
                .map(ToOwned::to_owned);
            if ConversationIdentityService::repair_single_chat_channel(
                &mut conv,
                &current_user_id,
                peer_hint.as_deref(),
            ) {
                self.conversations.save_one(&conv).await?;
            }
        }

        msg.channel_id = conv.channel_id.clone();
        msg.conversation_type = conv.conversation_type.to_proto_int();
        if conv.conversation_type.is_group_chat_conversation() {
            let participant_ids = conv
                .participants
                .iter()
                .map(|p| p.user_id.trim())
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            let member_ids = if participant_ids.is_empty() {
                conv.ext
                    .get("group_member_ids")
                    .cloned()
                    .unwrap_or_default()
            } else {
                participant_ids
            };
            if !member_ids.trim().is_empty() {
                msg.extra
                    .entry("group_member_ids".to_string())
                    .or_insert(member_ids);
            }
        }

        if conversation::is_single_chat_conversation(conversation_id) && msg.channel_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "单聊缺少对方 user_id（channel_id）。请先调用 conversation.get_one(对端, Single) 以补全会话行",
            ));
        }

        Ok(msg)
    }

    pub async fn create_text(
        &self,
        conversation_id: &str,
        text: &str,
        mention_all: bool,
    ) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let msg = MessageBuilderService::build_text(
            conversation_id,
            &sender_id,
            text,
            None,
            mention_all,
        )?;
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
        let msg = MessageBuilderService::build_thread_reply(
            conversation_id,
            &sender_id,
            thread_id,
            text,
        )?;
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

    pub async fn create_location(&self, request: CreateLocationRequest) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let conversation_id = request.conversation_id.clone();
        let msg = MessageBuilderService::build_location(BuildLocationRequest {
            conversation_id: request.conversation_id,
            sender_id,
            longitude: request.longitude,
            latitude: request.latitude,
            address: request.address,
            title: request.title,
            zoom: request.zoom,
            snapshot_url: request.snapshot_url,
            snapshot_local_path: request.snapshot_local_path,
            channel_id: None,
        })?;
        self.apply_conversation_routing(&conversation_id, msg).await
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
        let msg = MessageBuilderService::build_card(BuildCardRequest {
            conversation_id: conversation_id.to_string(),
            sender_id,
            id: id.to_string(),
            card_type: ct.to_string(),
            title: title.unwrap_or("").to_string(),
            subtitle: subtitle.unwrap_or("").to_string(),
            avatar: avatar.unwrap_or("").to_string(),
            channel_id: None,
        })?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_sticker(&self, request: CreateStickerRequest) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let conversation_id = request.conversation_id.clone();
        let msg = MessageBuilderService::build_sticker_with(BuildStickerRequest {
            conversation_id: request.conversation_id,
            sender_id,
            sticker_id: request.sticker_id,
            channel_id: None,
            package_id: request.package_id,
            url: request.url,
            width: request.width.unwrap_or(0),
            height: request.height.unwrap_or(0),
            sticker_format: request.sticker_format,
        })?;
        self.apply_conversation_routing(&conversation_id, msg).await
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
        let msg = MessageBuilderService::build_link_card(BuildLinkCardRequest {
            conversation_id: conversation_id.to_string(),
            sender_id,
            url: url.to_string(),
            channel_id: None,
            title: title.map(ToOwned::to_owned),
            description: description.map(ToOwned::to_owned),
            thumbnail_url: thumbnail_url.map(ToOwned::to_owned),
            site_name: site_name.map(ToOwned::to_owned),
        })?;
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
        let msg = MessageBuilderService::build_mini_program(BuildMiniProgramRequest {
            conversation_id: conversation_id.to_string(),
            sender_id,
            app_id: app_id.to_string(),
            channel_id: None,
            title: title.map(ToOwned::to_owned),
            page_path: page_path.map(ToOwned::to_owned),
            thumbnail_url: thumbnail_url.map(ToOwned::to_owned),
            extra,
        })?;
        self.apply_conversation_routing(conversation_id, msg).await
    }

    pub async fn create_rich_doc(&self, request: CreateRichDocRequest) -> Result<IMMessage> {
        let sender_id = self.current_sender_id().await?;
        let conversation_id = request.conversation_id.clone();
        let msg = MessageBuilderService::build_rich_doc(BuildRichDocRequest {
            conversation_id: request.conversation_id,
            sender_id,
            doc_json: request.doc_json,
            content_schema: request.content_schema,
            plain_text: request.plain_text,
            channel_id: None,
            input_format: request.input_format,
            input_format_version: request.input_format_version,
            source_payload: request.source_payload,
            title: request.title,
            search_text: request.search_text,
            render_hints_json: request.render_hints_json,
        })?;
        self.apply_conversation_routing(&conversation_id, msg).await
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
        let msg = MessageBuilderService::build_schedule(BuildScheduleRequest {
            conversation_id: conversation_id.to_string(),
            sender_id,
            schedule_id: schedule_id.to_string(),
            title: title.to_string(),
            channel_id: None,
            start_time_ms,
            end_time_ms,
            participant_user_ids,
        })?;
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
