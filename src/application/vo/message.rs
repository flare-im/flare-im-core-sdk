//! 消息视图模型
//!
//! 用于 API 层返回消息信息
//!
//! 基于 protobuf Message 定义，提供完整的消息视图对象

use crate::domain::message::Message as DomainMessage;
use serde::{Deserialize, Serialize};

/// 消息视图模型
///
/// 用于 API 层返回消息信息
/// 基于 `flare_proto::Message` 定义，包含所有消息字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageVO {
    // ========== 消息头（路由索引层）==========
    /// 消息 ID（全局唯一）
    pub message_id: String,
    /// 会话 ID
    pub session_id: String,
    /// 客户端消息 ID（用于去重）
    pub client_msg_id: String,
    /// 发送者 ID
    pub sender_id: String,
    /// 消息来源（user/system/bot/admin）
    pub source: i32,
    /// 序列号（用于排序）
    pub seq: u64,
    /// 时间戳（毫秒）
    pub timestamp: i64,
    /// 会话类型（single/group/channel）
    pub session_type: i32,
    /// 消息类型
    pub message_type: i32,
    /// 业务类型（可选）
    pub business_type: String,

    // ========== 路由字段 ==========
    /// 接收者 ID（单聊时必需，群聊时为空）
    pub receiver_id: Option<String>,
    /// 通道 ID（群聊/频道时使用）
    pub channel_id: Option<String>,

    // ========== 消息体（业务内容层）==========
    /// 消息内容（序列化的 MessageContent）
    pub content: MessageContentVO,
    /// 内容子类型（plain_text/markdown/html/json）
    pub content_type: i32,
    /// 媒体附件列表
    pub attachments: Vec<MediaAttachmentVO>,
    /// 系统扩展字段
    pub extra: std::collections::HashMap<String, String>,
    /// 业务扩展字段
    pub attributes: std::collections::HashMap<String, String>,

    // ========== 消息状态（生命周期状态层）==========
    /// 消息状态（created/sent/delivered/read/failed/recalled）
    pub status: i32,
    /// 是否已撤回
    pub is_recalled: bool,
    /// 撤回时间（毫秒时间戳）
    pub recalled_at: Option<i64>,
    /// 撤回原因
    pub recall_reason: Option<String>,
    /// 是否阅后即焚
    pub is_burn_after_read: bool,
    /// 阅后即焚秒数
    pub burn_after_seconds: Option<i32>,
    /// 时间线信息
    pub timeline: Option<MessageTimelineVO>,
    /// 可见性状态（user_id -> visibility_status）
    pub visibility: std::collections::HashMap<String, i32>,
    /// 已读记录列表
    pub read_by: Vec<MessageReadRecordVO>,
    /// 反应列表
    pub reactions: Vec<ReactionVO>,
    /// 编辑历史列表
    pub edit_history: Vec<EditHistoryVO>,

    // ========== 上下文信息 ==========
    /// 租户上下文
    pub tenant: Option<TenantContextVO>,
    /// 审计上下文
    pub audit: Option<AuditContextVO>,

    // ========== 扩展信息 ==========
    /// 标签列表
    pub tags: Vec<String>,
    /// 离线推送信息
    pub offline_push_info: Option<OfflinePushInfoVO>,
}

/// 消息内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageContentVO {
    /// 内容类型（text/image/video/audio/file/location/card/quote/link_card/forward/custom/notification/typing/system_event）
    pub content_type: String,
    /// 文本内容（如果 content_type 为 text）
    pub text: Option<TextContentVO>,
    /// 图片内容（如果 content_type 为 image）
    pub image: Option<ImageContentVO>,
    /// 视频内容（如果 content_type 为 video）
    pub video: Option<VideoContentVO>,
    /// 语音内容（如果 content_type 为 audio）
    pub audio: Option<AudioContentVO>,
    /// 文件内容（如果 content_type 为 file）
    pub file: Option<FileContentVO>,
    /// 位置内容（如果 content_type 为 location）
    pub location: Option<LocationContentVO>,
    /// 名片内容（如果 content_type 为 card）
    pub card: Option<CardContentVO>,
    /// 引用内容（如果 content_type 为 quote）
    pub quote: Option<QuoteContentVO>,
    /// 链接卡片内容（如果 content_type 为 link_card）
    pub link_card: Option<LinkCardContentVO>,
    /// 转发内容（如果 content_type 为 forward）
    pub forward: Option<ForwardContentVO>,
    /// 自定义内容（如果 content_type 为 custom）
    pub custom: Option<CustomContentVO>,
    /// 通知内容（如果 content_type 为 notification）
    pub notification: Option<NotificationContentVO>,
}

/// 文本内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContentVO {
    /// 文本内容
    pub text: String,
    /// @提及列表
    pub mentions: Vec<MentionVO>,
}

/// @提及视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionVO {
    /// 提及类型（user/all/role/multi）
    pub mention_type: i32,
    /// 用户ID（type=user时）
    pub user_id: Option<String>,
    /// 用户ID列表（type=multi时）
    pub user_ids: Vec<String>,
    /// 角色ID（type=role时）
    pub role_id: Option<String>,
    /// 角色名称（type=role时）
    pub role_name: Option<String>,
    /// 起始位置（字符索引）
    pub start: i32,
    /// 长度（字符数）
    pub length: i32,
    /// 扩展字段
    pub metadata: std::collections::HashMap<String, String>,
}

/// 图片内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContentVO {
    /// 图片ID（关联MediaAttachment）
    pub image_id: String,
    /// 原图信息
    pub source: ImageInfoVO,
    /// 缩略图信息（可选）
    pub thumbnail: Option<ImageInfoVO>,
    /// 图片描述（可选）
    pub description: Option<String>,
}

/// 图片信息视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfoVO {
    /// 图片UUID
    pub uuid: String,
    /// 图片URL
    pub url: String,
    /// MIME类型
    pub mime_type: String,
    /// 文件大小（字节）
    pub size: i64,
    /// 宽度
    pub width: i32,
    /// 高度
    pub height: i32,
}

/// 视频内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoContentVO {
    /// 视频ID（关联MediaAttachment）
    pub video_id: String,
    /// 视频信息
    pub source: VideoInfoVO,
    /// 封面图信息（可选）
    pub cover: Option<ImageInfoVO>,
    /// 视频描述（可选）
    pub description: Option<String>,
}

/// 视频信息视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfoVO {
    /// 视频UUID
    pub uuid: String,
    /// 视频URL
    pub url: String,
    /// MIME类型
    pub mime_type: String,
    /// 文件大小（字节）
    pub size: i64,
    /// 时长（毫秒）
    pub duration_ms: i64,
    /// 宽度
    pub width: i32,
    /// 高度
    pub height: i32,
}

/// 语音内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioContentVO {
    /// 语音ID（关联MediaAttachment）
    pub audio_id: String,
    /// 语音信息
    pub source: AudioInfoVO,
    /// 语音描述（可选）
    pub description: Option<String>,
}

/// 语音信息视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfoVO {
    /// 语音UUID
    pub uuid: String,
    /// 语音URL
    pub url: String,
    /// MIME类型
    pub mime_type: String,
    /// 文件大小（字节）
    pub size: i64,
    /// 时长（毫秒）
    pub duration_ms: i64,
}

/// 文件内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContentVO {
    /// 文件ID（关联MediaAttachment）
    pub file_id: String,
    /// 文件名
    pub file_name: String,
    /// MIME类型
    pub mime_type: String,
    /// 文件大小（字节）
    pub file_size: i64,
    /// 文件URL
    pub url: String,
    /// 文件描述（可选）
    pub description: Option<String>,
}

/// 位置内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationContentVO {
    /// 经度
    pub longitude: f64,
    /// 纬度
    pub latitude: f64,
    /// 地址
    pub address: String,
    /// 位置描述（可选）
    pub description: Option<String>,
    /// POI ID（可选）
    pub poi_id: Option<String>,
}

/// 名片内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardContentVO {
    /// 用户ID
    pub user_id: String,
    /// 用户昵称
    pub nickname: String,
    /// 头像URL
    pub avatar_url: String,
    /// 个人简介（可选）
    pub description: Option<String>,
    /// 扩展字段
    pub extra: std::collections::HashMap<String, String>,
}

/// 引用内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteContentVO {
    /// 被引用的消息ID
    pub quoted_message_id: String,
    /// 被引用消息的发送者ID
    pub quoted_sender_id: String,
    /// 引用内容预览
    pub quoted_text_preview: String,
}

/// 链接卡片内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkCardContentVO {
    /// 链接地址
    pub url: String,
    /// 标题
    pub title: String,
    /// 描述
    pub description: String,
    /// 缩略图URL
    pub thumbnail_url: Option<String>,
    /// 网站名称
    pub site_name: Option<String>,
}

/// 转发内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardContentVO {
    /// 被转发的消息ID列表
    pub message_ids: Vec<String>,
    /// 转发原因（可选）
    pub forward_reason: Option<String>,
}

/// 自定义内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomContentVO {
    /// 内容类型（业务系统定义）
    pub content_type: String,
    /// 负载数据（base64编码）
    pub payload: String,
    /// 消息描述（可选）
    pub description: Option<String>,
    /// 元数据
    pub metadata: std::collections::HashMap<String, String>,
}

/// 通知内容视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationContentVO {
    /// 通知标题
    pub title: String,
    /// 通知内容
    pub body: String,
    /// 通知类型（业务系统定义）
    pub notification_type: String,
    /// 通知数据
    pub data: std::collections::HashMap<String, String>,
    /// 目标用户ID列表（可选）
    pub target_user_ids: Vec<String>,
    /// 目标角色ID（可选）
    pub target_role_id: Option<String>,
    /// 是否通知所有人
    pub notify_all: bool,
    /// 是否持久化
    pub persistent: bool,
    /// 是否在消息列表中显示
    pub show_in_list: bool,
    /// 是否显示角标
    pub show_badge: bool,
    /// 是否播放提示音
    pub play_sound: bool,
}

/// 媒体附件视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachmentVO {
    /// 文件ID
    pub file_id: String,
    /// 文件名
    pub file_name: String,
    /// MIME类型
    pub mime_type: String,
    /// 文件大小（字节）
    pub size: i64,
    /// 文件访问URL
    pub url: String,
    /// CDN加速URL（可选）
    pub cdn_url: Option<String>,
    /// 时长（毫秒，仅音频/视频）
    pub duration_ms: Option<i64>,
    /// 校验和（SHA256/MD5）
    pub checksum: Option<String>,
    /// 扩展元数据
    pub metadata: std::collections::HashMap<String, String>,
}

/// 消息时间线视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTimelineVO {
    /// 创建时间（毫秒时间戳）
    pub created_at: Option<i64>,
    /// 持久化时间（毫秒时间戳）
    pub persisted_at: Option<i64>,
    /// 送达时间（毫秒时间戳）
    pub delivered_at: Option<i64>,
    /// 已读时间（毫秒时间戳）
    pub read_at: Option<i64>,
}

/// 消息已读记录视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReadRecordVO {
    /// 用户ID
    pub user_id: String,
    /// 已读时间（毫秒时间戳）
    pub read_at: Option<i64>,
    /// 销毁时间（阅后即焚，毫秒时间戳）
    pub burned_at: Option<i64>,
}

/// 反应视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionVO {
    /// 表情符号（如 👍、❤️、😂）
    pub emoji: String,
    /// 用户ID列表
    pub user_ids: Vec<String>,
    /// 反应计数（冗余字段，等于user_ids长度）
    pub count: i32,
    /// 最后更新时间（毫秒时间戳）
    pub last_updated: Option<i64>,
    /// 创建时间（毫秒时间戳）
    pub created_at: Option<i64>,
}

/// 编辑历史视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditHistoryVO {
    /// 编辑版本号（从1开始递增）
    pub edit_version: i32,
    /// 编辑后的内容（简化为文本）
    pub content_preview: String,
    /// 编辑时间（毫秒时间戳）
    pub edited_at: Option<i64>,
    /// 编辑者ID
    pub editor_id: String,
    /// 编辑原因（可选）
    pub reason: Option<String>,
    /// 是否显示"已编辑"标记
    pub show_edited_mark: bool,
}

/// 租户上下文视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantContextVO {
    /// 租户ID
    pub tenant_id: String,
    /// 业务类型
    pub business_type: String,
    /// 环境（production/staging/development）
    pub environment: String,
    /// 组织ID
    pub organization_id: Option<String>,
    /// 标签
    pub labels: std::collections::HashMap<String, String>,
    /// 扩展属性
    pub attributes: std::collections::HashMap<String, String>,
}

/// 审计上下文视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditContextVO {
    /// 操作者ID
    pub actor_id: String,
    /// 操作者类型（user/service/tenant_admin/system/guest）
    pub actor_type: i32,
    /// 操作时间（毫秒时间戳）
    pub operated_at: Option<i64>,
    /// 操作原因
    pub reason: Option<String>,
    /// 扩展元数据
    pub metadata: std::collections::HashMap<String, String>,
}

/// 离线推送信息视图模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfflinePushInfoVO {
    /// 推送标题
    pub title: String,
    /// 推送内容
    pub desc: String,
    /// iOS推送铃声
    pub ios_push_sound: Option<String>,
    /// iOS是否增加角标
    pub ios_badge_count: bool,
    /// 签名信息
    pub signal_info: Option<String>,
}

// ============================================================================
// 转换实现
// ============================================================================

impl From<DomainMessage> for MessageVO {
    fn from(msg: DomainMessage) -> Self {
        let proto = msg.to_proto();
        Self::from_proto(proto)
    }
}

impl MessageVO {
    /// 从领域模型创建视图模型
    pub fn from_domain(msg: &DomainMessage) -> Self {
        let proto = msg.to_proto();
        Self::from_proto(proto)
    }

    /// 从 ProtoMessage 创建视图模型
    pub fn from_proto(proto: flare_proto::Message) -> Self {
        // 转换时间戳为毫秒
        let timestamp_ms = proto
            .timestamp
            .as_ref()
            .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
            .unwrap_or(0);

        // 转换消息内容
        let content = if let Some(ref proto_content) = proto.content {
            MessageContentVO::from_proto(proto_content)
        } else {
            MessageContentVO::empty()
        };

        // 转换媒体附件
        let attachments = proto
            .attachments
            .into_iter()
            .map(MediaAttachmentVO::from_proto)
            .collect();

        // 转换时间线
        let timeline = proto.timeline.map(MessageTimelineVO::from_proto);

        // 转换已读记录
        let read_by = proto
            .read_by
            .into_iter()
            .map(MessageReadRecordVO::from_proto)
            .collect();

        // 转换反应列表
        let reactions = proto
            .reactions
            .into_iter()
            .map(ReactionVO::from_proto)
            .collect();

        // 转换编辑历史
        let edit_history = proto
            .edit_history
            .into_iter()
            .map(EditHistoryVO::from_proto)
            .collect();

        // 转换租户上下文
        let tenant = proto.tenant.map(TenantContextVO::from_proto);

        // 转换审计上下文
        let audit = proto.audit.map(AuditContextVO::from_proto);

        // 转换离线推送信息
        let offline_push_info = proto.offline_push_info.map(OfflinePushInfoVO::from_proto);

        Self {
            message_id: proto.id,
            session_id: proto.session_id,
            client_msg_id: proto.client_msg_id,
            sender_id: proto.sender_id,
            source: proto.source as i32,
            seq: proto.seq,
            timestamp: timestamp_ms,
            session_type: proto.session_type as i32,
            message_type: proto.message_type as i32,
            business_type: proto.business_type,
            receiver_id: if proto.receiver_id.is_empty() {
                None
            } else {
                Some(proto.receiver_id)
            },
            channel_id: if proto.channel_id.is_empty() {
                None
            } else {
                Some(proto.channel_id)
            },
            content,
            content_type: proto.content_type as i32,
            attachments,
            extra: proto.extra,
            attributes: proto.attributes,
            status: proto.status as i32,
            is_recalled: proto.is_recalled,
            recalled_at: proto
                .recalled_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            recall_reason: if proto.recall_reason.is_empty() {
                None
            } else {
                Some(proto.recall_reason)
            },
            is_burn_after_read: proto.is_burn_after_read,
            burn_after_seconds: if proto.burn_after_seconds == 0 {
                None
            } else {
                Some(proto.burn_after_seconds)
            },
            timeline,
            visibility: proto
                .visibility
                .into_iter()
                .map(|(k, v)| (k, v as i32))
                .collect(),
            read_by,
            reactions,
            edit_history,
            tenant,
            audit,
            tags: proto.tags,
            offline_push_info,
        }
    }
}

impl MessageContentVO {
    fn empty() -> Self {
        Self {
            content_type: "text".to_string(),
            text: None,
            image: None,
            video: None,
            audio: None,
            file: None,
            location: None,
            card: None,
            quote: None,
            link_card: None,
            forward: None,
            custom: None,
            notification: None,
        }
    }

    fn from_proto(proto: &flare_proto::MessageContent) -> Self {
        use flare_proto::flare::common::v1::message_content::Content;

        match &proto.content {
            Some(Content::Text(text)) => Self {
                content_type: "text".to_string(),
                text: Some(TextContentVO::from_proto(text)),
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Image(img)) => Self {
                content_type: "image".to_string(),
                text: None,
                image: Some(ImageContentVO::from_proto(img)),
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Video(video)) => Self {
                content_type: "video".to_string(),
                text: None,
                image: None,
                video: Some(VideoContentVO::from_proto(video)),
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Audio(audio)) => Self {
                content_type: "audio".to_string(),
                text: None,
                image: None,
                video: None,
                audio: Some(AudioContentVO::from_proto(audio)),
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::File(file)) => Self {
                content_type: "file".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: Some(FileContentVO::from_proto(file)),
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Location(loc)) => Self {
                content_type: "location".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: Some(LocationContentVO::from_proto(loc)),
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Card(card)) => Self {
                content_type: "card".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: Some(CardContentVO::from_proto(card)),
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Quote(quote)) => Self {
                content_type: "quote".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: Some(QuoteContentVO::from_proto(quote)),
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::LinkCard(link_card)) => Self {
                content_type: "link_card".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: Some(LinkCardContentVO::from_proto(link_card)),
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::Forward(forward)) => Self {
                content_type: "forward".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: Some(ForwardContentVO::from_proto(forward)),
                custom: None,
                notification: None,
            },
            Some(Content::Custom(custom)) => Self {
                content_type: "custom".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: Some(CustomContentVO::from_proto(custom)),
                notification: None,
            },
            Some(Content::Notification(notif)) => Self {
                content_type: "notification".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: Some(NotificationContentVO::from_proto(notif)),
            },
            Some(Content::Typing(_)) => Self {
                content_type: "typing".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            Some(Content::SystemEvent(_)) => Self {
                content_type: "system_event".to_string(),
                text: None,
                image: None,
                video: None,
                audio: None,
                file: None,
                location: None,
                card: None,
                quote: None,
                link_card: None,
                forward: None,
                custom: None,
                notification: None,
            },
            None => Self::empty(),
        }
    }
}

// 为各个 VO 类型实现 From trait（简化实现，只实现关键类型）
impl TextContentVO {
    fn from_proto(proto: &flare_proto::TextContent) -> Self {
        Self {
            text: proto.text.clone(),
            mentions: proto.mentions.iter().map(MentionVO::from_proto).collect(),
        }
    }
}

impl MentionVO {
    fn from_proto(proto: &flare_proto::Mention) -> Self {
        Self {
            mention_type: proto.r#type as i32,
            user_id: if proto.user_id.is_empty() {
                None
            } else {
                Some(proto.user_id.clone())
            },
            user_ids: proto.user_ids.clone(),
            role_id: if proto.role_id.is_empty() {
                None
            } else {
                Some(proto.role_id.clone())
            },
            role_name: if proto.role_name.is_empty() {
                None
            } else {
                Some(proto.role_name.clone())
            },
            start: proto.start,
            length: proto.length,
            metadata: proto.metadata.clone(),
        }
    }
}

impl ImageContentVO {
    fn from_proto(proto: &flare_proto::ImageContent) -> Self {
        Self {
            image_id: proto.image_id.clone(),
            source: ImageInfoVO::from_proto(&proto.source.as_ref().unwrap_or(&Default::default())),
            thumbnail: proto.thumbnail.as_ref().map(|t| ImageInfoVO::from_proto(t)),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
        }
    }
}

impl ImageInfoVO {
    fn from_proto(proto: &flare_proto::ImageInfo) -> Self {
        Self {
            uuid: proto.uuid.clone(),
            url: proto.url.clone(),
            mime_type: proto.mime_type.clone(),
            size: proto.size,
            width: proto.width,
            height: proto.height,
        }
    }
}

impl VideoContentVO {
    fn from_proto(proto: &flare_proto::VideoContent) -> Self {
        Self {
            video_id: proto.video_id.clone(),
            source: VideoInfoVO::from_proto(&proto.source.as_ref().unwrap_or(&Default::default())),
            cover: proto.cover.as_ref().map(|c| ImageInfoVO::from_proto(c)),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
        }
    }
}

impl VideoInfoVO {
    fn from_proto(proto: &flare_proto::VideoInfo) -> Self {
        Self {
            uuid: proto.uuid.clone(),
            url: proto.url.clone(),
            mime_type: proto.mime_type.clone(),
            size: proto.size,
            duration_ms: proto.duration_ms,
            width: proto.width,
            height: proto.height,
        }
    }
}

impl AudioContentVO {
    fn from_proto(proto: &flare_proto::AudioContent) -> Self {
        Self {
            audio_id: proto.audio_id.clone(),
            source: AudioInfoVO::from_proto(&proto.source.as_ref().unwrap_or(&Default::default())),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
        }
    }
}

impl AudioInfoVO {
    fn from_proto(proto: &flare_proto::AudioInfo) -> Self {
        Self {
            uuid: proto.uuid.clone(),
            url: proto.url.clone(),
            mime_type: proto.mime_type.clone(),
            size: proto.size,
            duration_ms: proto.duration_ms,
        }
    }
}

impl FileContentVO {
    fn from_proto(proto: &flare_proto::FileContent) -> Self {
        Self {
            file_id: proto.file_id.clone(),
            file_name: proto.file_name.clone(),
            mime_type: proto.mime_type.clone(),
            file_size: proto.file_size,
            url: proto.url.clone(),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
        }
    }
}

impl LocationContentVO {
    fn from_proto(proto: &flare_proto::LocationContent) -> Self {
        Self {
            longitude: proto.longitude,
            latitude: proto.latitude,
            address: proto.address.clone(),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
            poi_id: if proto.poi_id.is_empty() {
                None
            } else {
                Some(proto.poi_id.clone())
            },
        }
    }
}

impl CardContentVO {
    fn from_proto(proto: &flare_proto::CardContent) -> Self {
        Self {
            user_id: proto.user_id.clone(),
            nickname: proto.nickname.clone(),
            avatar_url: proto.avatar_url.clone(),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
            extra: proto.extra.clone(),
        }
    }
}

impl QuoteContentVO {
    fn from_proto(proto: &flare_proto::flare::common::v1::QuoteContent) -> Self {
        Self {
            quoted_message_id: proto.quoted_message_id.clone(),
            quoted_sender_id: proto.quoted_sender_id.clone(),
            quoted_text_preview: proto.quoted_text_preview.clone(),
        }
    }
}

impl LinkCardContentVO {
    fn from_proto(proto: &flare_proto::flare::common::v1::LinkCardContent) -> Self {
        Self {
            url: proto.url.clone(),
            title: proto.title.clone(),
            description: proto.description.clone(),
            thumbnail_url: if proto.thumbnail_url.is_empty() {
                None
            } else {
                Some(proto.thumbnail_url.clone())
            },
            site_name: if proto.site_name.is_empty() {
                None
            } else {
                Some(proto.site_name.clone())
            },
        }
    }
}

impl ForwardContentVO {
    fn from_proto(proto: &flare_proto::ForwardContent) -> Self {
        Self {
            message_ids: proto.message_ids.clone(),
            forward_reason: if proto.forward_reason.is_empty() {
                None
            } else {
                Some(proto.forward_reason.clone())
            },
        }
    }
}

impl CustomContentVO {
    fn from_proto(proto: &flare_proto::CustomContent) -> Self {
        Self {
            content_type: proto.r#type.clone(),
            payload: base64::encode(&proto.payload),
            description: if proto.description.is_empty() {
                None
            } else {
                Some(proto.description.clone())
            },
            metadata: proto.metadata.clone(),
        }
    }
}

impl NotificationContentVO {
    fn from_proto(proto: &flare_proto::NotificationContent) -> Self {
        Self {
            title: proto.title.clone(),
            body: proto.body.clone(),
            notification_type: proto.notification_type.clone(),
            data: proto.data.clone(),
            target_user_ids: proto.target_user_ids.clone(),
            target_role_id: if proto.target_role_id.is_empty() {
                None
            } else {
                Some(proto.target_role_id.clone())
            },
            notify_all: proto.notify_all,
            persistent: proto.persistent,
            show_in_list: proto.show_in_list,
            show_badge: proto.show_badge,
            play_sound: proto.play_sound,
        }
    }
}

impl MediaAttachmentVO {
    fn from_proto(proto: flare_proto::MediaAttachment) -> Self {
        Self {
            file_id: proto.file_id,
            file_name: proto.file_name,
            mime_type: proto.mime_type,
            size: proto.size,
            url: proto.url,
            cdn_url: if proto.cdn_url.is_empty() {
                None
            } else {
                Some(proto.cdn_url)
            },
            duration_ms: if proto.duration_ms == 0 {
                None
            } else {
                Some(proto.duration_ms)
            },
            checksum: if proto.checksum.is_empty() {
                None
            } else {
                Some(proto.checksum)
            },
            metadata: proto.metadata,
        }
    }
}

impl MessageTimelineVO {
    fn from_proto(proto: flare_proto::MessageTimeline) -> Self {
        Self {
            created_at: proto
                .created_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            persisted_at: proto
                .persisted_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            delivered_at: proto
                .delivered_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            read_at: proto
                .read_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
        }
    }
}

impl MessageReadRecordVO {
    fn from_proto(proto: flare_proto::MessageReadRecord) -> Self {
        Self {
            user_id: proto.user_id,
            read_at: proto
                .read_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            burned_at: proto
                .burned_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
        }
    }
}

impl ReactionVO {
    fn from_proto(proto: flare_proto::flare::common::v1::Reaction) -> Self {
        Self {
            emoji: proto.emoji,
            user_ids: proto.user_ids,
            count: proto.count,
            last_updated: proto
                .last_updated
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            created_at: proto
                .created_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
        }
    }
}

impl EditHistoryVO {
    fn from_proto(proto: flare_proto::flare::common::v1::EditHistory) -> Self {
        // 简化实现：将编辑后的内容转换为文本预览
        let content_preview = match &proto.content {
            Some(content) => match &content.content {
                Some(flare_proto::flare::common::v1::message_content::Content::Text(text)) => {
                    text.text.clone()
                }
                _ => "[已编辑]".to_string(),
            },
            None => "[已编辑]".to_string(),
        };

        Self {
            edit_version: proto.edit_version,
            content_preview,
            edited_at: proto
                .edited_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            editor_id: proto.editor_id,
            reason: if proto.reason.is_empty() {
                None
            } else {
                Some(proto.reason)
            },
            show_edited_mark: proto.show_edited_mark,
        }
    }
}

impl TenantContextVO {
    fn from_proto(proto: flare_proto::TenantContext) -> Self {
        Self {
            tenant_id: proto.tenant_id,
            business_type: proto.business_type,
            environment: proto.environment,
            organization_id: if proto.organization_id.is_empty() {
                None
            } else {
                Some(proto.organization_id)
            },
            labels: proto.labels,
            attributes: proto.attributes,
        }
    }
}

impl AuditContextVO {
    fn from_proto(proto: flare_proto::AuditContext) -> Self {
        Self {
            actor_id: proto
                .actor
                .as_ref()
                .map(|a| a.actor_id.clone())
                .unwrap_or_default(),
            actor_type: proto.actor.as_ref().map(|a| a.r#type as i32).unwrap_or(0),
            operated_at: proto
                .operated_at
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            reason: if proto.reason.is_empty() {
                None
            } else {
                Some(proto.reason)
            },
            metadata: proto.metadata,
        }
    }
}

impl OfflinePushInfoVO {
    fn from_proto(proto: flare_proto::OfflinePushInfo) -> Self {
        Self {
            title: proto.title,
            desc: proto.desc,
            ios_push_sound: if proto.ios_push_sound.is_empty() {
                None
            } else {
                Some(proto.ios_push_sound)
            },
            ios_badge_count: proto.ios_badge_count,
            signal_info: if proto.signal_info.is_empty() {
                None
            } else {
                Some(proto.signal_info)
            },
        }
    }
}
