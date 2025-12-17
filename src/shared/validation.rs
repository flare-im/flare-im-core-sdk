//! 消息验证模块
//!
//! 提供消息相关的输入验证和边界条件检查

use anyhow::{Result, anyhow};
use flare_proto::MessageContent;
use std::collections::HashMap;

/// 消息验证器
pub struct MessageValidator;

impl MessageValidator {
    /// 验证消息内容
    pub fn validate_message_content(content: &MessageContent) -> Result<()> {
        match &content.content {
            Some(content) => Self::validate_content_by_type(content)?,
            None => return Err(anyhow!("Message content cannot be empty")),
        }

        // 验证扩展字段
        if content.extensions.len() > 10 {
            return Err(anyhow!("Too many extensions, maximum allowed is 10"));
        }

        Ok(())
    }

    /// 验证会话ID
    pub fn validate_session_id(session_id: &str) -> Result<()> {
        if session_id.is_empty() {
            return Err(anyhow!("Session ID cannot be empty"));
        }

        if session_id.len() > 128 {
            return Err(anyhow!(
                "Session ID too long, maximum length is 128 characters"
            ));
        }

        // 检查非法字符
        if session_id.contains(|c| matches!(c, '\0' | '\n' | '\r')) {
            return Err(anyhow!("Session ID contains invalid characters"));
        }

        Ok(())
    }

    /// 验证消息ID
    pub fn validate_message_id(message_id: &str) -> Result<()> {
        if message_id.is_empty() {
            return Err(anyhow!("Message ID cannot be empty"));
        }

        if message_id.len() > 256 {
            return Err(anyhow!(
                "Message ID too long, maximum length is 256 characters"
            ));
        }

        Ok(())
    }

    /// 验证消息ID列表
    pub fn validate_message_ids(message_ids: &[String]) -> Result<()> {
        if message_ids.is_empty() {
            return Err(anyhow!("Message ID list cannot be empty"));
        }

        if message_ids.len() > 1000 {
            return Err(anyhow!("Too many message IDs, maximum allowed is 1000"));
        }

        // 验证每个消息ID
        for (index, message_id) in message_ids.iter().enumerate() {
            Self::validate_message_id(message_id)
                .map_err(|e| anyhow!("Invalid message ID at index {}: {}", index, e))?;
        }

        // 检查重复
        let mut unique_ids = std::collections::HashSet::new();
        for message_id in message_ids {
            if !unique_ids.insert(message_id) {
                return Err(anyhow!("Duplicate message ID found: {}", message_id));
            }
        }

        Ok(())
    }

    /// 验证撤回时间限制
    pub fn validate_recall_time_limit(
        sent_timestamp: &prost_types::Timestamp,
        time_limit: i64,
    ) -> Result<()> {
        let sent_time = sent_timestamp.seconds as i64;
        let now = chrono::Utc::now().timestamp();
        let elapsed_seconds = now - sent_time;

        if elapsed_seconds < 0 {
            return Err(anyhow!("Message timestamp is in the future"));
        }

        if elapsed_seconds > time_limit {
            return Err(anyhow!(
                "Message recall time limit exceeded. Limit: {}s, Elapsed: {}s",
                time_limit,
                elapsed_seconds
            ));
        }

        Ok(())
    }

    /// 验证文本内容
    fn validate_text_content(text: &str) -> Result<()> {
        if text.is_empty() {
            return Err(anyhow!("Text content cannot be empty"));
        }

        if text.len() > 10_000 {
            return Err(anyhow!(
                "Text content too long, maximum length is 10,000 characters"
            ));
        }

        // 检查控制字符
        if text
            .chars()
            .any(|c| c.is_control() && c != '\t' && c != '\n' && c != '\r')
        {
            return Err(anyhow!("Text content contains invalid control characters"));
        }

        Ok(())
    }

    /// 验证图片内容
    fn validate_image_content(image: &flare_proto::common::ImageContent) -> Result<()> {
        if image.image_id.is_empty() {
            return Err(anyhow!("Image ID cannot be empty"));
        }

        // 检查图片尺寸合理性
        if let Some(source) = &image.source {
            if source.width > 100_000 || source.height > 100_000 {
                return Err(anyhow!("Image dimensions too large"));
            }

            if source.size > 100 * 1024 * 1024 {
                // 100MB
                return Err(anyhow!("Image file size too large, maximum is 100MB"));
            }
        }

        Ok(())
    }

    /// 验证视频内容
    fn validate_video_content(video: &flare_proto::common::VideoContent) -> Result<()> {
        if video.video_id.is_empty() {
            return Err(anyhow!("Video ID cannot be empty"));
        }

        // 检查视频时长和尺寸
        if let Some(source) = &video.source {
            if source.duration_ms > 2 * 60 * 60 * 1000 {
                // 2小时
                return Err(anyhow!("Video duration too long, maximum is 2 hours"));
            }

            if source.size > 1024 * 1024 * 1024 {
                // 1GB
                return Err(anyhow!("Video file size too large, maximum is 1GB"));
            }
        }

        Ok(())
    }

    /// 验证音频内容
    fn validate_audio_content(audio: &flare_proto::common::AudioContent) -> Result<()> {
        if audio.audio_id.is_empty() {
            return Err(anyhow!("Audio ID cannot be empty"));
        }

        // 检查音频时长和大小
        if let Some(source) = &audio.source {
            if source.duration_ms > 60 * 60 * 1000 {
                // 1小时
                return Err(anyhow!("Audio duration too long, maximum is 1 hour"));
            }

            if source.size > 100 * 1024 * 1024 {
                // 100MB
                return Err(anyhow!("Audio file size too large, maximum is 100MB"));
            }
        }

        Ok(())
    }

    /// 验证文件内容
    fn validate_file_content(file: &flare_proto::common::FileContent) -> Result<()> {
        if file.file_id.is_empty() {
            return Err(anyhow!("File ID cannot be empty"));
        }

        if file.file_name.is_empty() {
            return Err(anyhow!("File name cannot be empty"));
        }

        if file.file_name.len() > 255 {
            return Err(anyhow!(
                "File name too long, maximum length is 255 characters"
            ));
        }

        // 检查文件名中的非法字符
        let invalid_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
        if file.file_name.contains(|c| invalid_chars.contains(&c)) {
            return Err(anyhow!("File name contains invalid characters"));
        }

        if file.file_size > 2 * 1024 * 1024 * 1024 {
            // 2GB
            return Err(anyhow!("File size too large, maximum is 2GB"));
        }

        Ok(())
    }

    /// 验证扩展字段
    pub fn validate_attributes(attributes: &HashMap<String, String>) -> Result<()> {
        if attributes.len() > 50 {
            return Err(anyhow!("Too many attributes, maximum allowed is 50"));
        }

        for (key, value) in attributes {
            if key.is_empty() {
                return Err(anyhow!("Attribute key cannot be empty"));
            }

            if key.len() > 100 {
                return Err(anyhow!(
                    "Attribute key too long, maximum length is 100 characters"
                ));
            }

            if value.len() > 1000 {
                return Err(anyhow!(
                    "Attribute value too long, maximum length is 1000 characters"
                ));
            }
        }

        Ok(())
    }

    /// 根据内容类型验证具体内容
    fn validate_content_by_type(
        content: &flare_proto::common::message_content::Content,
    ) -> Result<()> {
        match content {
            flare_proto::common::message_content::Content::Text(text) => {
                Self::validate_text_content(&text.text)?;

                // 验证@提及
                for (index, mention) in text.mentions.iter().enumerate() {
                    Self::validate_mention(mention)
                        .map_err(|e| anyhow!("Invalid mention at index {}: {}", index, e))?;
                }
            }
            flare_proto::common::message_content::Content::Image(image) => {
                Self::validate_image_content(image)?;
            }
            flare_proto::common::message_content::Content::Video(video) => {
                Self::validate_video_content(video)?;
            }
            flare_proto::common::message_content::Content::Audio(audio) => {
                Self::validate_audio_content(audio)?;
            }
            flare_proto::common::message_content::Content::File(file) => {
                Self::validate_file_content(file)?;
            }
            flare_proto::common::message_content::Content::Location(location) => {
                Self::validate_location_content(location)?;
            }
            flare_proto::common::message_content::Content::Card(card) => {
                Self::validate_card_content(card)?;
            }
            flare_proto::common::message_content::Content::Quote(quote) => {
                if quote.quoted_message_id.is_empty() {
                    return Err(anyhow!("Quoted message ID cannot be empty"));
                }
            }
            flare_proto::common::message_content::Content::Forward(forward) => {
                Self::validate_message_ids(&forward.message_ids)?;
            }
            flare_proto::common::message_content::Content::Custom(custom) => {
                Self::validate_custom_content(custom)?;
            }
            flare_proto::common::message_content::Content::LinkCard(_link_card) => {
                // LinkCard validation can be added later
            }
            flare_proto::common::message_content::Content::Notification(notification) => {
                Self::validate_notification_content(notification)?;
            }
            flare_proto::common::message_content::Content::Typing(_) => {
                // Typing content is temporary, minimal validation needed
            }
            flare_proto::common::message_content::Content::SystemEvent(_) => {
                // System event content is temporary, minimal validation needed
            }
        }

        Ok(())
    }

    /// 验证位置内容
    fn validate_location_content(location: &flare_proto::common::LocationContent) -> Result<()> {
        // 检查经纬度范围
        if location.longitude < -180.0 || location.longitude > 180.0 {
            return Err(anyhow!("Invalid longitude, must be between -180 and 180"));
        }

        if location.latitude < -90.0 || location.latitude > 90.0 {
            return Err(anyhow!("Invalid latitude, must be between -90 and 90"));
        }

        Ok(())
    }

    /// 验证名片内容
    fn validate_card_content(card: &flare_proto::common::CardContent) -> Result<()> {
        if card.user_id.is_empty() {
            return Err(anyhow!("Card user ID cannot be empty"));
        }

        if !card.nickname.is_empty() {
            if card.nickname.len() > 50 {
                return Err(anyhow!(
                    "Card nickname too long, maximum length is 50 characters"
                ));
            }
        }

        Ok(())
    }

    /// 验证自定义内容
    fn validate_custom_content(custom: &flare_proto::common::CustomContent) -> Result<()> {
        if custom.r#type.is_empty() {
            return Err(anyhow!("Custom content type cannot be empty"));
        }

        if custom.r#type.len() > 100 {
            return Err(anyhow!(
                "Custom content type too long, maximum length is 100 characters"
            ));
        }

        if custom.payload.len() > 10 * 1024 * 1024 {
            // 10MB
            return Err(anyhow!("Custom content payload too large, maximum is 10MB"));
        }

        Self::validate_attributes(&custom.metadata)?;

        Ok(())
    }

    /// 验证通知内容
    fn validate_notification_content(
        notification: &flare_proto::common::NotificationContent,
    ) -> Result<()> {
        if notification.notification_type.is_empty() {
            return Err(anyhow!("Notification type cannot be empty"));
        }

        if notification.title.len() > 100 {
            return Err(anyhow!(
                "Notification title too long, maximum length is 100 characters"
            ));
        }

        if notification.body.len() > 500 {
            return Err(anyhow!(
                "Notification body too long, maximum length is 500 characters"
            ));
        }

        // 验证目标用户列表
        if notification.target_user_ids.len() > 1000 {
            return Err(anyhow!("Too many target users, maximum allowed is 1000"));
        }

        Ok(())
    }

    /// 验证@提及
    fn validate_mention(mention: &flare_proto::common::Mention) -> Result<()> {
        use flare_proto::common::MentionType;

        match mention.r#type() {
            MentionType::Unspecified => {
                return Err(anyhow!("Mention type cannot be unspecified"));
            }
            MentionType::User => {
                if mention.user_id.is_empty() {
                    return Err(anyhow!("User mention requires user_id"));
                }
            }
            MentionType::All => {
                // @all 无需额外验证
            }
            MentionType::Role => {
                if mention.role_id.is_empty() {
                    return Err(anyhow!("Role mention requires role_id"));
                }
            }
            MentionType::Multi => {
                if mention.user_ids.is_empty() {
                    return Err(anyhow!("Multi mention requires user_ids"));
                }
            }
        }

        Ok(())
    }

    /// 验证表情反应
    pub fn validate_reaction(emoji: &str) -> Result<()> {
        if emoji.is_empty() {
            return Err(anyhow!("Reaction emoji cannot be empty"));
        }

        if emoji.len() > 100 {
            return Err(anyhow!(
                "Reaction emoji too long, maximum length is 100 characters"
            ));
        }

        // 验证是否为有效的emoji
        if !Self::is_valid_emoji(emoji) {
            return Err(anyhow!("Invalid emoji"));
        }

        Ok(())
    }

    /// 检查是否为有效的emoji
    fn is_valid_emoji(s: &str) -> bool {
        // 简单的emoji验证逻辑
        // 实际项目中可以使用更精确的emoji库
        s.chars().any(|c| {
            (c as u32) >= 0x1F600 && (c as u32) <= 0x1F64F || // Emoticons
            (c as u32) >= 0x1F300 && (c as u32) <= 0x1F5FF || // Misc Symbols and Pictographs
            (c as u32) >= 0x1F680 && (c as u32) <= 0x1F6FF || // Transport and Map
            (c as u32) >= 0x1F1E0 && (c as u32) <= 0x1F1FF || // Flags
            (c as u32) >= 0x2600 && (c as u32) <= 0x26FF ||    // Misc symbols
            (c as u32) >= 0x2700 && (c as u32) <= 0x27BF ||    // Dingbats
            (c as u32) >= 0xFE00 && (c as u32) <= 0xFE0F ||    // Variation Selectors
            (c as u32) >= 0x1F900 && (c as u32) <= 0x1F9FF ||    // Supplemental Symbols and Pictographs
            (c as u32) >= 0x1F018 && (c as u32) <= 0x1F270 // Various emoji
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_proto::common::{MessageContent, TextContent, message_content::Content};

    #[test]
    fn test_validate_empty_session_id() {
        let result = MessageValidator::validate_session_id("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_long_session_id() {
        let long_id = "a".repeat(129);
        let result = MessageValidator::validate_session_id(&long_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_valid_session_id() {
        let result = MessageValidator::validate_session_id("session_123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_text_content() {
        let text_content = TextContent {
            text: "Hello, world!".to_string(),
            mentions: vec![],
        };

        let content = MessageContent {
            content: Some(Content::Text(text_content)),
            extensions: vec![],
        };

        let result = MessageValidator::validate_message_content(&content);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_emoji() {
        assert!(MessageValidator::validate_reaction("👍").is_ok());
        assert!(MessageValidator::validate_reaction("❤️").is_ok());
        assert!(MessageValidator::validate_reaction("").is_err());
        assert!(MessageValidator::validate_reaction("abc").is_err());
    }
}
