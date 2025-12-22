//! 消息转换器
//!
//! 负责 Proto 消息和 Domain 消息之间的转换
//! 对标微信、Telegram、飞书的生产级别实现

use crate::domain::message::*;
use crate::domain::conversation::*;
use prost::Message as ProstMessage;
use anyhow::Result;
use std::collections::HashMap;

/// 消息转换器
pub struct MessageConverter;

impl MessageConverter {
    /// 从 Proto Message 转换为 Domain Message
    pub fn from_proto(proto: &flare_proto::flare::common::v1::Message) -> Result<Message> {
        // 转换消息状态（proto.status 是 i32）
        use flare_proto::flare::common::v1::MessageStatus;
        let state = match proto.status {
            1 => MessageState::Created,  // MESSAGE_STATUS_CREATED
            2 => MessageState::Sent,     // MESSAGE_STATUS_SENT
            3 => MessageState::Delivered, // MESSAGE_STATUS_DELIVERED
            4 => MessageState::Read,     // MESSAGE_STATUS_READ
            5 => MessageState::Failed,    // MESSAGE_STATUS_FAILED
            6 => MessageState::Recalled, // MESSAGE_STATUS_RECALLED
            _ => MessageState::Created,
        };
        
        // 转换消息来源（proto.source 是 i32）
        use crate::domain::message::MessageSource as DomainMessageSource;
        let source = match proto.source {
            1 => DomainMessageSource::User,    // MESSAGE_SOURCE_USER
            2 => DomainMessageSource::System,  // MESSAGE_SOURCE_SYSTEM
            3 => DomainMessageSource::Bot,     // MESSAGE_SOURCE_BOT
            4 => DomainMessageSource::Admin,   // MESSAGE_SOURCE_ADMIN
            _ => DomainMessageSource::User,
        };
        
        // 转换会话类型（proto.conversation_type 是 i32）
        let conversation_type = match proto.conversation_type {
            1 => ConversationType::Single,   // CONVERSATION_TYPE_SINGLE
            2 => ConversationType::Group,     // CONVERSATION_TYPE_GROUP
            3 => ConversationType::Channel,  // CONVERSATION_TYPE_CHANNEL
            _ => ConversationType::Single,
        };
        
        // 转换消息类型（proto.message_type 是 i32）
        let message_type = match proto.message_type {
            1 => MessageType::Text,         // MESSAGE_TYPE_TEXT
            2 => MessageType::Image,        // MESSAGE_TYPE_IMAGE
            3 => MessageType::Video,        // MESSAGE_TYPE_VIDEO
            4 => MessageType::Audio,        // MESSAGE_TYPE_AUDIO
            5 => MessageType::File,         // MESSAGE_TYPE_FILE
            6 => MessageType::Location,     // MESSAGE_TYPE_LOCATION
            7 => MessageType::Card,         // MESSAGE_TYPE_CARD
            100 => MessageType::Custom,     // MESSAGE_TYPE_CUSTOM
            101 => MessageType::Notification, // MESSAGE_TYPE_NOTIFICATION
            _ => MessageType::Text,
        };
        
        // 转换内容类型（proto.content_type 是 i32）
        let content_type = match proto.content_type {
            1 => ContentType::PlainText,   // CONTENT_TYPE_PLAIN_TEXT
            2 => ContentType::Markdown,    // CONTENT_TYPE_MARKDOWN
            3 => ContentType::Html,        // CONTENT_TYPE_HTML
            4 => ContentType::Json,        // CONTENT_TYPE_JSON
            _ => ContentType::PlainText,
        };
        
        // 序列化消息内容
        let content = proto.content.as_ref()
            .map(|c| {
                let mut buf = Vec::new();
                c.encode(&mut buf)?;
                Ok::<Vec<u8>, anyhow::Error>(buf)
            })
            .transpose()?
            .unwrap_or_default();
        
        // 转换时间线
        let timeline = if let Some(t) = &proto.timeline {
            MessageTimeline {
                created_at: t.created_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
                persisted_at: t.persisted_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()),
                delivered_at: t.delivered_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()),
                read_at: t.read_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()),
            }
        } else {
            MessageTimeline {
                created_at: proto.timestamp.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
                persisted_at: None,
                delivered_at: None,
                read_at: None,
            }
        };
        
        // 转换已读记录
        let read_by = proto.read_by.iter()
            .map(|r| MessageReadRecord {
                user_id: r.user_id.clone(),
                read_at: r.read_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
                burned_at: r.burned_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()),
            })
            .collect();
        
        // 转换反应
        let reactions = proto.reactions.iter()
            .map(|r| Reaction {
                emoji: r.emoji.clone(),
                user_ids: r.user_ids.clone(),
                count: r.count,
                last_updated: r.last_updated.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
                created_at: r.created_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
            })
            .collect();
        
        // 转换编辑历史
        let edit_history = proto.edit_history.iter()
            .map(|e| {
                let mut buf = Vec::new();
                e.content.as_ref().map(|c| c.encode(&mut buf));
                EditHistory {
                    edit_version: e.edit_version,
                    content: buf,
                    edited_at: e.edited_at.as_ref()
                        .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                        .unwrap_or_else(|| chrono::Utc::now()),
                    editor_id: e.editor_id.clone(),
                    reason: if e.reason.is_empty() { None } else { Some(e.reason.clone()) },
                    show_edited_mark: e.show_edited_mark,
                }
            })
            .collect();
        
        // 转换租户上下文
        // 注意：user_id 应该从消息的 sender_id 获取（对于接收到的消息）
        // 对于发送的消息，user_id 应该在创建消息时设置
        let tenant = if let Some(t) = &proto.tenant {
            TenantContext {
                tenant_id: t.tenant_id.clone(),
                // 对于接收到的消息，user_id 应该从 sender_id 获取
                // 但这里暂时使用空字符串，实际使用时应该从 Session 或消息上下文获取
                user_id: String::new(), // 注意：实际使用时应该从 Session 获取当前用户 ID
            }
        } else {
            return Err(anyhow::anyhow!("Tenant context is required"));
        };
        
        // 转换审计上下文
        let audit = proto.audit.as_ref().map(|a| {
            let operator_id = a.actor.as_ref()
                .map(|actor| actor.actor_id.clone())
                .unwrap_or_default();
            
            // 从 actor.type 获取操作类型
            let operation_type = a.actor.as_ref()
                .map(|actor| {
                    match actor.r#type {
                        1 => "USER".to_string(),    // ACTOR_TYPE_USER
                        2 => "SYSTEM".to_string(),  // ACTOR_TYPE_SYSTEM
                        3 => "BOT".to_string(),     // ACTOR_TYPE_BOT
                        4 => "ADMIN".to_string(),   // ACTOR_TYPE_ADMIN
                        _ => "UNKNOWN".to_string(),
                    }
                })
                .unwrap_or_default();
            
            AuditContext {
                operator_id,
                operation_type,
                operation_time: a.operated_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
                ip_address: a.metadata.get("ip_address").cloned(),
            }
        });
        
        // 转换时间戳
        let timestamp = proto.timestamp.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now());
        
        Ok(Message {
            id: proto.id.clone(),
            conversation_id: proto.conversation_id.clone(),
            client_msg_id: proto.client_msg_id.clone(),
            sender_id: proto.sender_id.clone(),
            source,
            seq: if proto.seq > 0 { Some(proto.seq) } else { None },
            timestamp,
            conversation_type,
            message_type,
            business_type: if proto.business_type.is_empty() { None } else { Some(proto.business_type.clone()) },
            receiver_id: if proto.receiver_id.is_empty() { None } else { Some(proto.receiver_id.clone()) },
            channel_id: if proto.channel_id.is_empty() { None } else { Some(proto.channel_id.clone()) },
            content,
            content_type,
            attachments: Self::convert_attachments(&proto.attachments)?,
            extra: proto.extra.clone(),
            attributes: proto.attributes.clone(),
            state,
            is_recalled: proto.is_recalled,
            recalled_at: proto.recalled_at.as_ref()
                .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()),
            recall_reason: if proto.recall_reason.is_empty() { None } else { Some(proto.recall_reason.clone()) },
            is_burn_after_read: proto.is_burn_after_read,
            burn_after_seconds: if proto.burn_after_seconds > 0 { Some(proto.burn_after_seconds) } else { None },
            timeline: timeline.clone(),
            visibility: Self::convert_visibility(&proto.visibility)?,
            read_by,
            reactions,
            edit_history,
            tenant,
            audit,
            tags: proto.tags.clone(),
            offline_push_info: Self::convert_offline_push_info(&proto.offline_push_info)?,
            version: 0,
            created_at: timeline.created_at,
            updated_at: timeline.created_at,
        })
    }
    
    /// 从 Domain Message 转换为 Proto Message
    pub fn to_proto(msg: &Message) -> Result<flare_proto::flare::common::v1::Message> {
        // 转换消息状态（使用枚举值）
        let status = match msg.state {
            MessageState::Created => 1,   // MESSAGE_STATUS_CREATED
            MessageState::Sent => 2,      // MESSAGE_STATUS_SENT
            MessageState::Delivered => 3,  // MESSAGE_STATUS_DELIVERED
            MessageState::Read => 4,       // MESSAGE_STATUS_READ
            MessageState::Failed => 5,     // MESSAGE_STATUS_FAILED
            MessageState::Recalled => 6,   // MESSAGE_STATUS_RECALLED
        };
        
        // 转换消息来源
        let source = match msg.source {
            MessageSource::User => 1,    // MESSAGE_SOURCE_USER
            MessageSource::System => 2,  // MESSAGE_SOURCE_SYSTEM
            MessageSource::Bot => 3,     // MESSAGE_SOURCE_BOT
            MessageSource::Admin => 4,   // MESSAGE_SOURCE_ADMIN
        };
        
        // 转换会话类型
        let conversation_type = match msg.conversation_type {
            ConversationType::Single => 1,   // CONVERSATION_TYPE_SINGLE
            ConversationType::Group => 2,     // CONVERSATION_TYPE_GROUP
            ConversationType::Channel => 3,   // CONVERSATION_TYPE_CHANNEL
        };
        
        // 转换消息类型
        let message_type = match msg.message_type {
            MessageType::Text => 1,          // MESSAGE_TYPE_TEXT
            MessageType::Image => 2,         // MESSAGE_TYPE_IMAGE
            MessageType::Video => 3,         // MESSAGE_TYPE_VIDEO
            MessageType::Audio => 4,         // MESSAGE_TYPE_AUDIO
            MessageType::File => 5,          // MESSAGE_TYPE_FILE
            MessageType::Location => 6,       // MESSAGE_TYPE_LOCATION
            MessageType::Card => 7,          // MESSAGE_TYPE_CARD
            MessageType::Custom => 100,      // MESSAGE_TYPE_CUSTOM
            MessageType::Notification => 101, // MESSAGE_TYPE_NOTIFICATION
        };
        
        // 转换内容类型
        let content_type = match msg.content_type {
            ContentType::PlainText => 1,  // CONTENT_TYPE_PLAIN_TEXT
            ContentType::Markdown => 2,   // CONTENT_TYPE_MARKDOWN
            ContentType::Html => 3,       // CONTENT_TYPE_HTML
            ContentType::Json => 4,       // CONTENT_TYPE_JSON
        };
        
        // 反序列化消息内容
        let content = flare_proto::flare::common::v1::MessageContent::decode(msg.content.as_slice())?;
        
        // 转换时间戳
        let timestamp = Some(prost_types::Timestamp {
            seconds: msg.timestamp.timestamp(),
            nanos: msg.timestamp.timestamp_subsec_nanos() as i32,
        });
        
        // 转换时间线
        let timeline = Some(flare_proto::flare::common::v1::MessageTimeline {
            created_at: Some(prost_types::Timestamp {
                seconds: msg.timeline.created_at.timestamp(),
                nanos: msg.timeline.created_at.timestamp_subsec_nanos() as i32,
            }),
            persisted_at: msg.timeline.persisted_at.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            }),
            delivered_at: msg.timeline.delivered_at.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            }),
            read_at: msg.timeline.read_at.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            }),
        });
        
        // 转换已读记录
        let read_by = msg.read_by.iter()
            .map(|r| flare_proto::flare::common::v1::MessageReadRecord {
                user_id: r.user_id.clone(),
                read_at: Some(prost_types::Timestamp {
                    seconds: r.read_at.timestamp(),
                    nanos: r.read_at.timestamp_subsec_nanos() as i32,
                }),
                burned_at: r.burned_at.map(|dt| prost_types::Timestamp {
                    seconds: dt.timestamp(),
                    nanos: dt.timestamp_subsec_nanos() as i32,
                }),
            })
            .collect();
        
        // 转换反应
        let reactions = msg.reactions.iter()
            .map(|r| flare_proto::flare::common::v1::Reaction {
                emoji: r.emoji.clone(),
                user_ids: r.user_ids.clone(),
                count: r.count,
                last_updated: Some(prost_types::Timestamp {
                    seconds: r.last_updated.timestamp(),
                    nanos: r.last_updated.timestamp_subsec_nanos() as i32,
                }),
                created_at: Some(prost_types::Timestamp {
                    seconds: r.created_at.timestamp(),
                    nanos: r.created_at.timestamp_subsec_nanos() as i32,
                }),
            })
            .collect();
        
        // 转换编辑历史
        let edit_history = msg.edit_history.iter()
            .map(|e| {
                let content = flare_proto::flare::common::v1::MessageContent::decode(e.content.as_slice())
                    .unwrap_or_default();
                flare_proto::flare::common::v1::EditHistory {
                    edit_version: e.edit_version,
                    content: Some(content),
                    edited_at: Some(prost_types::Timestamp {
                        seconds: e.edited_at.timestamp(),
                        nanos: e.edited_at.timestamp_subsec_nanos() as i32,
                    }),
                    editor_id: e.editor_id.clone(),
                    reason: e.reason.clone().unwrap_or_default(),
                    show_edited_mark: e.show_edited_mark,
                }
            })
            .collect();
        
        // 转换租户上下文
        // 注意：business_type、environment、organization_id 应该从配置或消息的 extra 中获取
        let tenant = Some(flare_proto::flare::common::v1::TenantContext {
            tenant_id: msg.tenant.tenant_id.clone(),
            business_type: msg.extra.get("business_type").cloned().unwrap_or_default(),
            environment: msg.extra.get("environment").cloned().unwrap_or_default(),
            organization_id: msg.extra.get("organization_id").cloned().unwrap_or_default(),
            labels: HashMap::new(), // 可以从 msg.extra 中提取
            attributes: HashMap::new(), // 可以从 msg.attributes 中提取
        });
        
        // 转换审计上下文
        let audit = msg.audit.as_ref().map(|a| {
            use flare_proto::flare::common::v1::ActorContext;
            flare_proto::flare::common::v1::AuditContext {
                actor: Some(ActorContext {
                    actor_id: a.operator_id.clone(),
                    r#type: 1, // ACTOR_TYPE_USER
                    roles: Vec::new(),
                    attributes: HashMap::new(),
                }),
                operated_at: Some(prost_types::Timestamp {
                    seconds: a.operation_time.timestamp(),
                    nanos: a.operation_time.timestamp_subsec_nanos() as i32,
                }),
                reason: String::new(), // 注意：AuditContext 中没有 reason 字段，可以从 metadata 获取
                metadata: {
                    let mut metadata = HashMap::new();
                    if let Some(ip) = &a.ip_address {
                        metadata.insert("ip_address".to_string(), ip.clone());
                    }
                    metadata.insert("operation_type".to_string(), a.operation_type.clone());
                    metadata
                },
            }
        });
        
        Ok(flare_proto::flare::common::v1::Message {
            id: msg.id.clone(),
            conversation_id: msg.conversation_id.clone(),
            client_msg_id: msg.client_msg_id.clone(),
            sender_id: msg.sender_id.clone(),
            source,
            seq: msg.seq.unwrap_or(0),
            timestamp,
            conversation_type,
            message_type,
            business_type: msg.business_type.clone().unwrap_or_default(),
            receiver_id: msg.receiver_id.clone().unwrap_or_default(),
            channel_id: msg.channel_id.clone().unwrap_or_default(),
            content: Some(content),
            content_type,
            attachments: Self::convert_attachments_to_proto(&msg.attachments)?,
            extra: msg.extra.clone(),
            attributes: msg.attributes.clone(),
            status,
            is_recalled: msg.is_recalled,
            recalled_at: msg.recalled_at.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            }),
            recall_reason: msg.recall_reason.clone().unwrap_or_default(),
            is_burn_after_read: msg.is_burn_after_read,
            burn_after_seconds: msg.burn_after_seconds.unwrap_or(0),
            timeline,
            visibility: Self::convert_visibility_to_proto(&msg.visibility),
            read_by,
            reactions,
            edit_history,
            tenant,
            audit,
            tags: msg.tags.clone(),
            offline_push_info: Self::convert_offline_push_info_to_proto(&msg.offline_push_info),
            extensions: Vec::new(),
        })
    }
    
    /// 转换附件（从 Proto 到 Domain）
    fn convert_attachments(
        proto_attachments: &[flare_proto::common::MediaAttachment],
    ) -> Result<Vec<MediaAttachment>> {
        proto_attachments
            .iter()
            .map(|proto_att| {
                Ok(MediaAttachment {
                    attachment_id: proto_att.file_id.clone(),
                    attachment_type: proto_att.mime_type.clone(),
                    url: proto_att.url.clone(),
                    size: proto_att.size as u64,
                    mime_type: proto_att.mime_type.clone(),
                    metadata: proto_att.metadata.clone(),
                })
            })
            .collect()
    }
    
    /// 转换附件（从 Domain 到 Proto）
    fn convert_attachments_to_proto(
        attachments: &[MediaAttachment],
    ) -> Result<Vec<flare_proto::common::MediaAttachment>> {
        attachments
            .iter()
            .map(|att| {
                Ok(flare_proto::common::MediaAttachment {
                    file_id: att.attachment_id.clone(),
                    file_name: att.metadata.get("file_name").cloned().unwrap_or_default(),
                    mime_type: att.mime_type.clone(),
                    size: att.size as i64,
                    url: att.url.clone(),
                    cdn_url: att.metadata.get("cdn_url").cloned().unwrap_or_default(),
                    duration_ms: att.metadata.get("duration_ms")
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0),
                    checksum: att.metadata.get("checksum").cloned().unwrap_or_default(),
                    metadata: att.metadata.clone(),
                })
            })
            .collect()
    }
    
    /// 转换可见性（从 Proto 到 Domain）
    fn convert_visibility(
        proto_visibility: &std::collections::HashMap<String, i32>,
    ) -> Result<HashMap<String, VisibilityStatus>> {
        proto_visibility
            .iter()
            .map(|(user_id, status)| {
                let visibility_status = match *status {
                    1 => VisibilityStatus::Visible,   // VISIBILITY_STATUS_VISIBLE
                    2 => VisibilityStatus::Hidden,    // VISIBILITY_STATUS_HIDDEN
                    3 => VisibilityStatus::Deleted,   // VISIBILITY_STATUS_DELETED
                    _ => VisibilityStatus::Visible,
                };
                Ok((user_id.clone(), visibility_status))
            })
            .collect()
    }
    
    /// 转换可见性（从 Domain 到 Proto）
    fn convert_visibility_to_proto(
        visibility: &HashMap<String, VisibilityStatus>,
    ) -> HashMap<String, i32> {
        visibility
            .iter()
            .map(|(user_id, status)| {
                let status_value = match status {
                    VisibilityStatus::Visible => 1,  // VISIBILITY_STATUS_VISIBLE
                    VisibilityStatus::Hidden => 2,   // VISIBILITY_STATUS_HIDDEN
                    VisibilityStatus::Deleted => 3,  // VISIBILITY_STATUS_DELETED
                };
                (user_id.clone(), status_value)
            })
            .collect()
    }
    
    /// 转换离线推送信息（从 Proto 到 Domain）
    fn convert_offline_push_info(
        proto_push_info: &Option<flare_proto::flare::common::v1::OfflinePushInfo>,
    ) -> Result<Option<OfflinePushInfo>> {
        if let Some(push_info) = proto_push_info {
            Ok(Some(OfflinePushInfo {
                title: push_info.title.clone(),
                desc: push_info.desc.clone(),
                ios_push_sound: if push_info.ios_push_sound.is_empty() {
                    None
                } else {
                    Some(push_info.ios_push_sound.clone())
                },
                ios_badge_count: push_info.ios_badge_count,
                signal_info: if push_info.signal_info.is_empty() {
                    None
                } else {
                    Some(push_info.signal_info.clone())
                },
            }))
        } else {
            Ok(None)
        }
    }
    
    /// 转换离线推送信息（从 Domain 到 Proto）
    fn convert_offline_push_info_to_proto(
        push_info: &Option<OfflinePushInfo>,
    ) -> Option<flare_proto::flare::common::v1::OfflinePushInfo> {
        push_info.as_ref().map(|info| {
            flare_proto::flare::common::v1::OfflinePushInfo {
                title: info.title.clone(),
                desc: info.desc.clone(),
                ios_push_sound: info.ios_push_sound.clone().unwrap_or_default(),
                ios_badge_count: info.ios_badge_count,
                signal_info: info.signal_info.clone().unwrap_or_default(),
            }
        })
    }
}

/// 会话转换器
pub struct ConversationConverter;

impl ConversationConverter {
    /// 从 Proto ConversationSummary 转换为 Domain Conversation
    pub fn from_proto_summary(proto: &flare_proto::flare::common::v1::ConversationSummary) -> Result<Conversation> {
        let mut conv = Conversation::new(
            proto.conversation_id.clone(),
            proto.conversation_type.clone(),
        );
        
        conv.business_type = if proto.business_type.is_empty() { None } else { Some(proto.business_type.clone()) };
        conv.display_name = proto.display_name.clone();
        conv.avatar_url = if proto.avatar_url.is_empty() { None } else { Some(proto.avatar_url.clone()) };
        conv.unread_count = proto.unread_count;
        conv.max_seq = proto.max_seq;
        conv.last_read_seq = proto.last_read_seq;
        conv.is_muted = proto.is_muted;
        conv.is_pinned = proto.is_pinned;
        conv.is_muted_detail = proto.is_muted_detail;
        conv.mute_until = proto.mute_until.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default());
        conv.labels = proto.labels.clone();
        conv.ext = proto.metadata.clone();
        
        // 转换最后一条消息预览
        if let Some(preview) = &proto.last_message {
            conv.last_message = Some(MessagePreview {
                message_id: preview.message_id.clone(),
                sender_id: preview.sender_id.clone(),
                message_type: format!("{:?}", preview.r#type), // TODO: 转换消息类型枚举
                text: preview.text.clone(),
                time: preview.time.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
            });
        }
        
        // 转换时间
        conv.created_at = proto.created_at.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now());
        conv.updated_at = proto.updated_at.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now());
        
        Ok(conv)
    }
    
    /// 从 Proto ConversationDetail 转换为 Domain Conversation
    pub fn from_proto_detail(proto: &flare_proto::flare::common::v1::ConversationDetail) -> Result<Conversation> {
        let mut conv = Conversation::new(
            proto.conversation_id.clone(),
            proto.conversation_type.clone(),
        );
        
        conv.business_type = if proto.business_type.is_empty() { None } else { Some(proto.business_type.clone()) };
        conv.attributes = proto.attributes.clone();
        conv.display_name = proto.display_name.clone();
        conv.avatar_url = if proto.avatar_url.is_empty() { None } else { Some(proto.avatar_url.clone()) };
        conv.description = if proto.description.is_empty() { None } else { Some(proto.description.clone()) };
        conv.extended_config = proto.extended_config.clone();
        
        // 转换参与者
        conv.participants = proto.participants.iter()
            .map(|p| ConversationParticipant {
                user_id: p.user_id.clone(),
                roles: p.roles.clone(),
                muted: p.muted,
                pinned: p.pinned,
                attributes: p.attributes.clone(),
                joined_at: p.joined_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
                    .unwrap_or_else(|| chrono::Utc::now()),
                nickname: if p.nickname.is_empty() { None } else { Some(p.nickname.clone()) },
            })
            .collect();
        
        // 转换可见性（proto.visibility 是 i32）
        conv.visibility = match proto.visibility {
            1 => ConversationVisibility::Private,  // CONVERSATION_VISIBILITY_PRIVATE
            2 => ConversationVisibility::Tenant,   // CONVERSATION_VISIBILITY_TENANT
            3 => ConversationVisibility::Public,    // CONVERSATION_VISIBILITY_PUBLIC
            _ => ConversationVisibility::Private,
        };
        
        // 转换生命周期状态（proto.lifecycle_state 是 i32）
        conv.lifecycle_state = match proto.lifecycle_state {
            1 => ConversationLifecycleState::Active,     // CONVERSATION_LIFECYCLE_ACTIVE
            2 => ConversationLifecycleState::Suspended, // CONVERSATION_LIFECYCLE_SUSPENDED
            3 => ConversationLifecycleState::Archived, // CONVERSATION_LIFECYCLE_ARCHIVED
            4 => ConversationLifecycleState::Deleted,   // CONVERSATION_LIFECYCLE_DELETED
            _ => ConversationLifecycleState::Active,
        };
        
        // 转换策略
        if let Some(policy) = &proto.policy {
            conv.policy = Some(ConversationPolicy {
                conflict_resolution: match policy.conflict_resolution {
                    1 => ConflictResolution::Exclusive,          // CONFLICT_RESOLUTION_EXCLUSIVE
                    2 => ConflictResolution::PlatformExclusive,  // CONFLICT_RESOLUTION_PLATFORM_EXCLUSIVE
                    3 => ConflictResolution::Coexist,            // CONFLICT_RESOLUTION_COEXIST
                    4 => ConflictResolution::ForceLogout,        // CONFLICT_RESOLUTION_FORCE_LOGOUT
                    _ => ConflictResolution::Coexist,
                },
                max_devices: if policy.max_devices > 0 { Some(policy.max_devices) } else { None },
                allow_anonymous: policy.allow_anonymous,
                allow_history_sync: policy.allow_history_sync,
                metadata: policy.metadata.clone(),
                allow_message_search: policy.allow_message_search,
                allow_file_transfer: policy.allow_file_transfer,
            });
        }
        
        // 转换设备在线状态
        if let Some(presence) = &proto.presence {
            conv.presence = Some(DevicePresence {
                device_id: presence.device_id.clone(),
                device_platform: presence.device_platform.clone(),
                state: match presence.state {
                    1 => DeviceState::Online,    // DEVICE_STATE_ONLINE
                    2 => DeviceState::Offline,   // DEVICE_STATE_OFFLINE
                    3 => DeviceState::Conflict,   // DEVICE_STATE_CONFLICT
                    _ => DeviceState::Offline,
                },
                last_seen_at: presence.last_seen_at.as_ref()
                    .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()),
                device_name: if presence.device_name.is_empty() { None } else { Some(presence.device_name.clone()) },
                ip_address: if presence.ip_address.is_empty() { None } else { Some(presence.ip_address.clone()) },
            });
        }
        
        // 转换公告
        conv.announcement = if proto.announcement.is_empty() { None } else { Some(proto.announcement.clone()) };
        conv.announcement_updated_at = proto.announcement_updated_at.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default());
        conv.announcement_updated_by = if proto.announcement_updated_by.is_empty() { None } else { Some(proto.announcement_updated_by.clone()) };
        
        // 转换时间
        conv.created_at = proto.created_at.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now());
        conv.updated_at = proto.updated_at.as_ref()
            .map(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default())
            .unwrap_or_else(|| chrono::Utc::now());
        
        Ok(conv)
    }
}
