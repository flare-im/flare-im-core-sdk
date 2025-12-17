//! 消息管理 API 实现

#[cfg(test)]
mod message_helper;
#[cfg(not(test))]
#[path = "message_helper.rs"]
mod message_helper;
use message_helper::{build_domain_message, create_base_builder};

use crate::api::FlareIMClient;
use crate::api::traits::MessageApi;
use crate::application::commands::message::{
    AddReactionCommand, DeleteMessageCommand, EditMessageCommand, FavoriteMessageCommand,
    ForwardMessageCommand, PinMessageCommand, RecallMessageCommand, RemoveReactionCommand,
    SendMessageCommand, UnfavoriteMessageCommand, UnpinMessageCommand,
};
use crate::application::queries::*;
use crate::application::vo::MessageVO;
use crate::domain::message::Message as DomainMessage;
use crate::domain::{MessageId, MessageType, SessionId, UserId};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;

impl MessageApi for FlareIMClient {
    fn create_text_message(
        &self,
        session_id: &str,
        text: &str,
        mentions: Option<Vec<String>>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        let mut text_content = flare_proto::TextContent {
            text: text.to_string(),
            mentions: vec![],
        };

        if let Some(mention_user_ids) = mentions {
            for user_id in mention_user_ids {
                text_content.mentions.push(flare_proto::Mention {
                    r#type: flare_proto::common::MentionType::User as i32,
                    user_id,
                    user_ids: vec![],
                    role_id: String::new(),
                    role_name: String::new(),
                    metadata: std::collections::HashMap::new(),
                    start: 0,
                    length: text.len() as i32,
                });
            }
        }

        builder = builder.text(text_content.text.clone());
        build_domain_message(builder)
    }

    fn create_text_at_message(
        &self,
        session_id: &str,
        text: &str,
        user_ids: Vec<String>,
    ) -> Result<DomainMessage> {
        self.create_text_message(session_id, text, Some(user_ids))
    }

    fn create_quote_message(
        &self,
        session_id: &str,
        quoted_message_id: &str,
        text: &str,
        preview_text: Option<String>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        builder = builder.text(text.to_string());

        // 设置回复信息
        builder = builder.metadata("reply_to".to_string(), quoted_message_id.to_string());
        if let Some(preview) = preview_text {
            builder = builder.metadata("reply_preview".to_string(), preview);
        }

        build_domain_message(builder)
    }

    fn create_location_message(
        &self,
        session_id: &str,
        latitude: f64,
        longitude: f64,
        address: Option<String>,
        description: Option<String>,
        poi_id: Option<String>,
    ) -> Result<DomainMessage> {
        // 位置消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let address_text = address.unwrap_or_else(|| format!("{},{}", latitude, longitude));
        let mut builder = create_base_builder(session_id, &user_id);

        builder = builder.text(format!("位置: {}", address_text));
        builder = builder.metadata("latitude".to_string(), latitude.to_string());
        builder = builder.metadata("longitude".to_string(), longitude.to_string());
        if let Some(desc) = description {
            builder = builder.metadata("description".to_string(), desc);
        }
        if let Some(poi) = poi_id {
            builder = builder.metadata("poi_id".to_string(), poi);
        }

        build_domain_message(builder)
    }

    fn create_card_message(
        &self,
        session_id: &str,
        title: &str,
        description: Option<String>,
        image_url: Option<String>,
    ) -> Result<DomainMessage> {
        // 卡片消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        let content = description.as_deref().unwrap_or(title);
        builder = builder.text(content.to_string());
        builder = builder.metadata("card_title".to_string(), title.to_string());
        if let Some(desc) = description {
            builder = builder.metadata("card_description".to_string(), desc);
        }
        if let Some(img) = image_url {
            builder = builder.metadata("card_image_url".to_string(), img);
        }

        build_domain_message(builder)
    }

    fn create_face_message(&self, session_id: &str, emoji: &str) -> Result<DomainMessage> {
        // 表情消息实际上就是文本消息，包含表情符号
        self.create_text_message(session_id, emoji, None)
    }

    fn create_custom_message(
        &self,
        session_id: &str,
        data: Vec<u8>,
        mime_type: &str,
    ) -> Result<DomainMessage> {
        // 自定义消息使用文本消息 + metadata 实现（实际数据需要base64编码）
        let user_id = self.user_id.blocking_read().clone();
        let data_base64 = base64::encode(&data);
        let mut builder =
            create_base_builder(session_id, &user_id).text("[自定义消息]".to_string());
        builder = builder.metadata("custom_data".to_string(), data_base64);
        builder = builder.metadata("mime_type".to_string(), mime_type.to_string());
        build_domain_message(builder)
    }

    async fn create_image_message_from_full_path(
        &self,
        session_id: &str,
        image_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        _options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        // 注意：此方法只创建消息对象，实际上传需要在发送时进行
        let user_id = self.user_id.read().await.clone();
        let path_str = image_path.as_ref().to_string_lossy().to_string();
        let mut builder = create_base_builder(session_id, &user_id);

        // 创建 ImageInfo 作为 source
        let image_info = flare_proto::ImageInfo {
            uuid: String::new(), // TODO: 生成 UUID
            url: path_str,
            mime_type: String::new(), // TODO: 从文件推断 MIME 类型
            size: 0,                  // TODO: 获取实际文件大小
            width: 0,                 // TODO: 获取实际宽度
            height: 0,                // TODO: 获取实际高度
        };

        let desc = description.clone().unwrap_or_default();
        let image_content = flare_proto::ImageContent {
            image_id: String::new(), // TODO: 生成或从上传服务获取
            source: Some(image_info),
            thumbnail: None,
            description: desc.clone(),
        };
        builder = builder.image(image_content);

        if let Some(ref desc) = description {
            builder = builder.metadata("description".to_string(), desc.clone());
        }

        build_domain_message(builder)
    }

    fn create_image_message_by_url(
        &self,
        session_id: &str,
        image_url: String,
        width: Option<i32>,
        height: Option<i32>,
        description: Option<String>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        // 创建 ImageInfo 作为 source
        let image_info = flare_proto::ImageInfo {
            uuid: String::new(), // TODO: 生成 UUID
            url: image_url,
            mime_type: String::new(), // TODO: 从 URL 推断 MIME 类型
            size: 0,                  // TODO: 获取实际文件大小
            width: width.unwrap_or(0),
            height: height.unwrap_or(0),
        };

        let image_content = flare_proto::ImageContent {
            image_id: String::new(), // TODO: 生成或从上传服务获取
            source: Some(image_info),
            thumbnail: None,
            description: description.unwrap_or_default(),
        };
        builder = builder.image(image_content);

        build_domain_message(builder)
    }

    #[cfg(target_arch = "wasm32")]
    async fn create_image_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        anyhow::bail!("create_image_message_by_file: Not implemented yet")
    }

    async fn create_sound_message_from_full_path(
        &self,
        session_id: &str,
        audio_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        _options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.read().await.clone();
        let path_str = audio_path.as_ref().to_string_lossy().to_string();
        let mut builder = create_base_builder(session_id, &user_id);

        // 创建 AudioInfo 作为 source
        let audio_info = flare_proto::AudioInfo {
            uuid: String::new(), // TODO: 生成 UUID
            url: path_str,
            mime_type: String::new(), // TODO: 从文件推断 MIME 类型
            size: 0,                  // TODO: 获取实际文件大小
            duration_ms: 0,           // TODO: 获取实际时长（毫秒）
        };

        let audio_content = flare_proto::AudioContent {
            audio_id: String::new(), // TODO: 生成或从上传服务获取
            source: Some(audio_info),
            description: description.unwrap_or_default(),
        };
        builder = builder.audio(audio_content);

        build_domain_message(builder)
    }

    fn create_sound_message_by_url(
        &self,
        session_id: &str,
        audio_url: String,
        duration: Option<i32>,
        description: Option<String>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        // 创建 AudioInfo 作为 source
        let audio_info = flare_proto::AudioInfo {
            uuid: String::new(), // TODO: 生成 UUID
            url: audio_url,
            mime_type: String::new(), // TODO: 从 URL 推断 MIME 类型
            size: 0,                  // TODO: 获取实际文件大小
            duration_ms: duration.map(|d| d as i64 * 1000).unwrap_or(0), // 转换为毫秒
        };

        let audio_content = flare_proto::AudioContent {
            audio_id: String::new(), // TODO: 生成或从上传服务获取
            source: Some(audio_info),
            description: description.unwrap_or_default(),
        };
        builder = builder.audio(audio_content);

        build_domain_message(builder)
    }

    #[cfg(target_arch = "wasm32")]
    async fn create_sound_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        anyhow::bail!("create_sound_message_by_file: Not implemented yet")
    }

    async fn create_video_message_from_full_path(
        &self,
        session_id: &str,
        video_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        _options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.read().await.clone();
        let path_str = video_path.as_ref().to_string_lossy().to_string();
        let mut builder = create_base_builder(session_id, &user_id);

        // 创建 VideoInfo 作为 source
        let video_info = flare_proto::VideoInfo {
            uuid: String::new(), // TODO: 生成 UUID
            url: path_str,
            mime_type: String::new(), // TODO: 从文件推断 MIME 类型
            size: 0,                  // TODO: 获取实际文件大小
            duration_ms: 0,           // TODO: 获取实际时长（毫秒）
            width: 0,                 // TODO: 获取实际宽度
            height: 0,                // TODO: 获取实际高度
        };

        let video_content = flare_proto::VideoContent {
            video_id: String::new(), // TODO: 生成或从上传服务获取
            source: Some(video_info),
            cover: None,
            description: description.unwrap_or_default(),
        };
        builder = builder.video(video_content);

        build_domain_message(builder)
    }

    fn create_video_message_by_url(
        &self,
        session_id: &str,
        video_url: String,
        duration: Option<i32>,
        width: Option<i32>,
        height: Option<i32>,
        description: Option<String>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        // 创建 VideoInfo 作为 source
        let video_info = flare_proto::VideoInfo {
            uuid: String::new(), // TODO: 生成 UUID
            url: video_url,
            mime_type: String::new(), // TODO: 从 URL 推断 MIME 类型
            size: 0,                  // TODO: 获取实际文件大小
            duration_ms: duration.map(|d| d as i64 * 1000).unwrap_or(0), // 转换为毫秒
            width: width.unwrap_or(0),
            height: height.unwrap_or(0),
        };

        let video_content = flare_proto::VideoContent {
            video_id: String::new(), // TODO: 生成或从上传服务获取
            source: Some(video_info),
            cover: None,
            description: description.unwrap_or_default(),
        };
        builder = builder.video(video_content);

        build_domain_message(builder)
    }

    #[cfg(target_arch = "wasm32")]
    async fn create_video_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        anyhow::bail!("create_video_message_by_file: Not implemented yet")
    }

    async fn create_file_message_from_full_path(
        &self,
        session_id: &str,
        file_path: impl AsRef<std::path::Path> + Send,
        description: Option<String>,
        _options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.read().await.clone();
        let path = file_path.as_ref();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let path_str = path.to_string_lossy().to_string();
        let mut builder = create_base_builder(session_id, &user_id);

        let file_content = flare_proto::FileContent {
            file_id: String::new(), // TODO: 生成或从上传服务获取
            file_name,
            mime_type: "application/octet-stream".to_string(),
            file_size: 0, // TODO: 获取实际文件大小
            url: path_str,
            description: description.unwrap_or_default(),
        };
        builder = builder.file(file_content);

        build_domain_message(builder)
    }

    fn create_file_message_by_url(
        &self,
        session_id: &str,
        file_url: String,
        file_name: String,
        file_size: i64,
        description: Option<String>,
    ) -> Result<DomainMessage> {
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id);

        let file_content = flare_proto::FileContent {
            file_id: String::new(), // TODO: 生成或从上传服务获取
            file_name,
            mime_type: "application/octet-stream".to_string(),
            file_size,
            url: file_url,
            description: description.unwrap_or_default(),
        };
        builder = builder.file(file_content);

        build_domain_message(builder)
    }

    #[cfg(target_arch = "wasm32")]
    async fn create_file_message_by_file(
        &self,
        session_id: &str,
        file: web_sys::File,
        description: Option<String>,
        options: Option<crate::infrastructure::storage::MediaUploadOptions>,
    ) -> Result<DomainMessage> {
        anyhow::bail!("create_file_message_by_file: Not implemented yet")
    }

    fn create_forward_message(
        &self,
        session_id: &str,
        message_ids: Vec<String>,
        merge: bool,
    ) -> Result<DomainMessage> {
        // 转发消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id)
            .text(format!("[转发{}条消息]", message_ids.len()));
        builder = builder.metadata("forward_message_ids".to_string(), message_ids.join(","));
        builder = builder.metadata("merge_forward".to_string(), merge.to_string());
        build_domain_message(builder)
    }

    fn create_merge_message(
        &self,
        session_id: &str,
        message_ids: Vec<String>,
    ) -> Result<DomainMessage> {
        self.create_forward_message(session_id, message_ids, true)
    }

    fn create_link_card_message(
        &self,
        session_id: &str,
        url: String,
        title: String,
        description: Option<String>,
        thumbnail_url: Option<String>,
        site_name: Option<String>,
    ) -> Result<DomainMessage> {
        // 链接卡片消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id).text(title.clone());
        builder = builder.metadata("link_url".to_string(), url);
        builder = builder.metadata("link_title".to_string(), title);
        if let Some(desc) = description {
            builder = builder.metadata("link_description".to_string(), desc);
        }
        if let Some(thumb) = thumbnail_url {
            builder = builder.metadata("link_thumbnail_url".to_string(), thumb);
        }
        if let Some(site) = site_name {
            builder = builder.metadata("link_site_name".to_string(), site);
        }
        build_domain_message(builder)
    }

    fn create_mini_program_message(
        &self,
        session_id: &str,
        app_id: String,
        page_path: String,
        title: String,
        description: Option<String>,
        thumbnail_url: Option<String>,
    ) -> Result<DomainMessage> {
        // 小程序消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id).text(title.clone());
        builder = builder.metadata("mini_program_app_id".to_string(), app_id);
        builder = builder.metadata("mini_program_page_path".to_string(), page_path);
        builder = builder.metadata("mini_program_title".to_string(), title);
        if let Some(desc) = description {
            builder = builder.metadata("mini_program_description".to_string(), desc);
        }
        if let Some(thumb) = thumbnail_url {
            builder = builder.metadata("mini_program_thumbnail_url".to_string(), thumb);
        }
        build_domain_message(builder)
    }

    fn create_vote_message(
        &self,
        session_id: &str,
        question: String,
        options: Vec<String>,
        allow_multiple: bool,
        expire_at: Option<prost_types::Timestamp>,
    ) -> Result<DomainMessage> {
        // 投票消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id).text(question.clone());
        builder = builder.metadata("vote_question".to_string(), question);
        builder = builder.metadata("vote_options".to_string(), options.join("|"));
        builder = builder.metadata(
            "vote_allow_multiple".to_string(),
            allow_multiple.to_string(),
        );
        if let Some(expire) = expire_at {
            builder = builder.metadata("vote_expire_at".to_string(), expire.seconds.to_string());
        }
        build_domain_message(builder)
    }

    fn create_task_message(
        &self,
        session_id: &str,
        title: String,
        description: Option<String>,
        assignee_id: Option<String>,
        due_date: Option<prost_types::Timestamp>,
        priority: Option<i32>,
    ) -> Result<DomainMessage> {
        // 任务消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id).text(title.clone());
        builder = builder.metadata("task_title".to_string(), title);
        if let Some(desc) = description {
            builder = builder.metadata("task_description".to_string(), desc);
        }
        if let Some(assignee) = assignee_id {
            builder = builder.metadata("task_assignee_id".to_string(), assignee);
        }
        if let Some(due) = due_date {
            builder = builder.metadata("task_due_date".to_string(), due.seconds.to_string());
        }
        if let Some(pri) = priority {
            builder = builder.metadata("task_priority".to_string(), pri.to_string());
        }
        build_domain_message(builder)
    }

    fn create_schedule_message(
        &self,
        session_id: &str,
        title: String,
        description: Option<String>,
        start_time: prost_types::Timestamp,
        end_time: prost_types::Timestamp,
        location: Option<String>,
        attendees: Option<Vec<String>>,
    ) -> Result<DomainMessage> {
        // 日程消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder = create_base_builder(session_id, &user_id).text(title.clone());
        builder = builder.metadata("schedule_title".to_string(), title);
        if let Some(desc) = description {
            builder = builder.metadata("schedule_description".to_string(), desc);
        }
        builder = builder.metadata(
            "schedule_start_time".to_string(),
            start_time.seconds.to_string(),
        );
        builder = builder.metadata(
            "schedule_end_time".to_string(),
            end_time.seconds.to_string(),
        );
        if let Some(loc) = location {
            builder = builder.metadata("schedule_location".to_string(), loc);
        }
        if let Some(atts) = attendees {
            builder = builder.metadata("schedule_attendees".to_string(), atts.join(","));
        }
        build_domain_message(builder)
    }

    fn create_announcement_message(
        &self,
        session_id: &str,
        title: String,
        content: String,
        pinned: bool,
        expire_at: Option<prost_types::Timestamp>,
    ) -> Result<DomainMessage> {
        // 公告消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder =
            create_base_builder(session_id, &user_id).text(format!("{}: {}", title, content));
        builder = builder.metadata("announcement_title".to_string(), title);
        builder = builder.metadata("announcement_content".to_string(), content);
        builder = builder.metadata("announcement_pinned".to_string(), pinned.to_string());
        if let Some(expire) = expire_at {
            builder = builder.metadata(
                "announcement_expire_at".to_string(),
                expire.seconds.to_string(),
            );
        }
        build_domain_message(builder)
    }

    fn create_notification_message(
        &self,
        session_id: &str,
        notification_type: String,
        title: String,
        body: String,
        data: Option<HashMap<String, String>>,
        target_user_ids: Option<Vec<String>>,
    ) -> Result<DomainMessage> {
        // 通知消息使用文本消息 + metadata 实现
        let user_id = self.user_id.blocking_read().clone();
        let mut builder =
            create_base_builder(session_id, &user_id).text(format!("{}: {}", title, body));
        builder = builder.metadata("notification_type".to_string(), notification_type);
        builder = builder.metadata("notification_title".to_string(), title);
        builder = builder.metadata("notification_body".to_string(), body);
        if let Some(data_map) = data {
            for (k, v) in data_map {
                builder = builder.metadata(format!("notification_data_{}", k), v);
            }
        }
        if let Some(user_ids) = target_user_ids {
            builder = builder.metadata(
                "notification_target_user_ids".to_string(),
                user_ids.join(","),
            );
        }
        build_domain_message(builder)
    }

    async fn send_message(
        &self,
        message: DomainMessage,
        receiver_id: Option<String>,
        channel_id: Option<String>,
    ) -> Result<String> {
        // 从 DomainMessage 转换为 SendMessageCommand
        let proto_message = message.to_proto();
        let content = proto_message
            .content
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Message content is required"))?;
        let message_type = MessageType::try_from(proto_message.message_type)
            .map_err(|_| anyhow::anyhow!("Invalid message type"))?;

        let cmd = SendMessageCommand {
            session_id: SessionId::new(proto_message.session_id),
            sender_id: UserId::new(proto_message.sender_id),
            receiver_id: receiver_id.map(UserId::new),
            channel_id,
            content,
            message_type,
            seq: None, // 序列号由服务端分配
        };

        self.message_command_handler
            .handle_send_message(cmd)
            .await
            .context("Failed to send message")
            .map(|id| id.to_string())
    }

    async fn recall_message(&self, message_id: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = RecallMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            reason: None,
        };
        self.message_command_handler
            .handle_recall_message(cmd)
            .await
            .context("Failed to recall message")
    }

    async fn recall_messages_batch(
        &self,
        message_ids: Vec<String>,
    ) -> Result<Vec<(String, Result<()>)>> {
        let mut results = Vec::new();
        for msg_id in message_ids {
            let result = self.recall_message(&msg_id).await;
            results.push((msg_id, result));
        }
        Ok(results)
    }

    async fn edit_message(&self, message_id: &str, new_content: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = EditMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            new_content: new_content.to_string(),
        };
        self.message_command_handler
            .handle_edit_message(cmd)
            .await
            .context("Failed to edit message")
    }

    async fn delete_message(
        &self,
        message_id: &str,
        delete_type: i32,
        notify_others: bool,
    ) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = DeleteMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            delete_type,
        };
        self.message_command_handler
            .handle_delete_message(cmd)
            .await
            .context("Failed to delete message")
    }

    async fn delete_messages_batch(
        &self,
        message_ids: Vec<String>,
        delete_type: i32,
    ) -> Result<Vec<(String, Result<()>)>> {
        let mut results = Vec::new();
        for msg_id in message_ids {
            let result = self.delete_message(&msg_id, delete_type, false).await;
            results.push((msg_id, result));
        }
        Ok(results)
    }

    async fn delete_local(&self, message_id: &str) -> Result<()> {
        use crate::domain::message::repository::MessageRepository;
        use crate::infrastructure::persistence::storage::MessageRepositoryImpl;
        let repo: Arc<dyn MessageRepository> =
            Arc::new(MessageRepositoryImpl::new(Arc::clone(&self.storage)));
        repo.delete(&MessageId::new(message_id.to_string()))
            .await
            .context("Failed to delete message from local storage")
    }

    async fn delete_local_batch(&self, message_ids: Vec<String>) -> Result<()> {
        for msg_id in message_ids {
            let _ = self.delete_local(&msg_id).await;
        }
        Ok(())
    }

    async fn clear_local(&self, session_id: &str) -> Result<usize> {
        self.storage
            .delete_all_messages(session_id)
            .await
            .context("Failed to clear local messages")
    }

    async fn clear(&self, session_id: &str) -> Result<usize> {
        self.clear_local(session_id).await
    }

    async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = AddReactionCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            emoji: emoji.to_string(),
        };
        self.message_command_handler
            .handle_add_reaction(cmd)
            .await
            .context("Failed to add reaction")
    }

    async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = RemoveReactionCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            emoji: emoji.to_string(),
        };
        self.message_command_handler
            .handle_remove_reaction(cmd)
            .await
            .context("Failed to remove reaction")
    }

    async fn forward_messages(
        &self,
        message_ids: Vec<String>,
        target_session_id: &str,
        _merge: bool,
    ) -> Result<Vec<String>> {
        // 转发多条消息
        let mut forwarded_ids = Vec::new();
        let user_id = self.user_id.read().await.clone();

        for msg_id in message_ids {
            let cmd = ForwardMessageCommand {
                message_id: MessageId::new(msg_id),
                target_session_id: SessionId::new(target_session_id.to_string()),
                sender_id: UserId::new(user_id.clone()),
            };
            match self
                .message_command_handler
                .handle_forward_message(cmd)
                .await
            {
                Ok(id) => forwarded_ids.push(id.to_string()),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to forward message");
                }
            }
        }

        Ok(forwarded_ids)
    }

    async fn forward_messages_batch(
        &self,
        message_ids: Vec<String>,
        target_session_ids: Vec<String>,
        merge: bool,
    ) -> Result<HashMap<String, Vec<String>>> {
        let mut results = HashMap::new();
        for target_session_id in target_session_ids {
            if let Ok(forwarded_ids) = self
                .forward_messages(message_ids.clone(), &target_session_id, merge)
                .await
            {
                results.insert(target_session_id, forwarded_ids);
            }
        }
        Ok(results)
    }

    async fn get_messages(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<MessageVO>> {
        let query = GetMessagesQuery {
            session_id: SessionId::new(session_id.to_string()),
            limit,
            before_message_id: cursor.map(MessageId::new),
        };

        let proto_messages = self
            .message_query_handler
            .handle_get_messages(query)
            .await
            .context("Failed to get messages")?;

        // 将 ProtoMessage 转换为 MessageVO
        Ok(proto_messages
            .into_iter()
            .map(MessageVO::from_proto)
            .collect())
    }

    async fn get_message(&self, message_id: &str) -> Result<Option<MessageVO>> {
        let query = GetMessageQuery {
            message_id: MessageId::new(message_id.to_string()),
        };
        let proto_message = self
            .message_query_handler
            .handle_get_message(query)
            .await
            .context("Failed to get message")?;

        Ok(proto_message.map(MessageVO::from_proto))
    }

    async fn get_messages_batch(&self, message_ids: Vec<String>) -> Result<Vec<MessageVO>> {
        let mut results = Vec::new();
        for msg_id in message_ids {
            if let Ok(Some(msg_vo)) = self.get_message(&msg_id).await {
                results.push(msg_vo);
            }
        }
        Ok(results)
    }

    async fn search(
        &self,
        query: &str,
        session_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MessageVO>> {
        let search_query = SearchMessagesQuery {
            keyword: query.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            limit: Some(limit),
        };
        let proto_messages = self
            .message_query_handler
            .handle_search_messages(search_query)
            .await
            .context("Failed to search messages")?;

        // 将 ProtoMessage 转换为 MessageVO
        let messages: Vec<MessageVO> = proto_messages
            .into_iter()
            .map(MessageVO::from_proto)
            .collect();
        Ok(messages)
    }

    #[cfg(feature = "extensions")]
    async fn get_messages_extended(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<Vec<crate::domain::message::ExtendedMessage>> {
        anyhow::bail!("get_messages_extended: Not implemented yet")
    }

    async fn retry(&self, message_id: &str) -> Result<()> {
        anyhow::bail!("retry: Not implemented yet")
    }

    async fn cancel_retry(&self, message_id: &str) -> Result<()> {
        anyhow::bail!("cancel_retry: Not implemented yet")
    }

    async fn get_retrying(&self) -> Vec<String> {
        Vec::new()
    }

    async fn pin(&self, message_id: &str, expire_at: Option<prost_types::Timestamp>) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = PinMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            expire_at,
        };
        self.message_command_handler
            .handle_pin_message(cmd)
            .await
            .context("Failed to pin message")
    }

    async fn unpin(&self, message_id: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = UnpinMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
        };
        self.message_command_handler
            .handle_unpin_message(cmd)
            .await
            .context("Failed to unpin message")
    }

    async fn favorite(
        &self,
        message_id: &str,
        tags: Option<Vec<String>>,
        note: Option<String>,
    ) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = FavoriteMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
            tags,
            note,
        };
        self.message_command_handler
            .handle_favorite_message(cmd)
            .await
            .context("Failed to favorite message")
    }

    async fn unfavorite(&self, message_id: &str) -> Result<()> {
        let user_id = self.user_id.read().await.clone();
        let cmd = UnfavoriteMessageCommand {
            message_id: MessageId::new(message_id.to_string()),
            user_id: UserId::new(user_id),
        };
        self.message_command_handler
            .handle_unfavorite_message(cmd)
            .await
            .context("Failed to unfavorite message")
    }

    async fn set_extension(
        &self,
        message_id: &str,
        extension: HashMap<String, String>,
    ) -> Result<()> {
        anyhow::bail!("set_extension: Not implemented yet")
    }
}
