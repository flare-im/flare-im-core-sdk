//! 统一转换层
//!
//! 负责所有格式转换：JSON ↔ Domain Model ↔ Protobuf
//! 对标微信、Telegram、飞书的生产级别实现

mod error;
mod traits;
mod registry;
mod json_domain;
mod domain_proto;
mod enum_helpers;
mod message_operation_converter;

pub use error::ConversionError;
pub use traits::{Converter, BatchConverter};
pub use registry::ConverterRegistry;
pub use json_domain::{MessageJsonConverter, ConversationJsonConverter};
pub use domain_proto::MessageProtoConverter;
pub use message_operation_converter::MessageOperationConverter;

use crate::domain::message::*;
use crate::domain::conversation::*;
use enum_helpers::*;

use anyhow::Result;
use std::collections::HashMap;
use flare_proto::MessageContentExt;

/// 消息转换器（保留用于向后兼容，内部使用新的转换器架构）
pub struct MessageConverter;

impl MessageConverter {
    /// 从 Proto Message 转换为 Domain Message（对齐 flare-proto common/message.proto：created_at 唯一业务时间）
    pub fn from_proto(proto: &flare_proto::flare::common::v1::Message) -> Result<Message> {
        let mut state = message_state::from_proto(proto.status);
        if state == MessageState::Created && !proto.server_id.is_empty() {
            state = MessageState::Sent;
        }
        let source = message_source::from_proto(proto.source);
        let conversation_type = conversation_type::from_proto(proto.conversation_type);
        let message_type = message_type::from_proto(proto.message_type);
        let content_type = content_type::from_proto(proto.content_type);
        let content = proto.content.as_ref()
            .map(|c| c.encode_to_bytes())
            .transpose()
            .map_err(|e| anyhow::anyhow!("Failed to encode MessageContent: {}", e))?
            .unwrap_or_default();
        let created_at = proto.created_at.as_ref()
            .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32))
            .unwrap_or_else(chrono::Utc::now);
        let timeline = MessageTimeline {
            created_at,
            persisted_at: None,
            delivered_at: None,
            read_at: None,
        };
        let is_recalled = state == MessageState::Recalled;
        let read_by: Vec<MessageReadRecord> = Vec::new();
        let reactions: Vec<Reaction> = Vec::new();
        let edit_history: Vec<crate::domain::message::EditHistory> = Vec::new();
        
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
        
        Ok(Message {
            server_id: Some(proto.server_id.clone()),
            conversation_id: Some(proto.conversation_id.clone()),
            client_msg_id: proto.client_msg_id.clone(),
            sender_id: proto.sender_id.clone(),
            source,
            seq: if proto.seq > 0 { Some(proto.seq) } else { None },
            timestamp: created_at,
            conversation_type,
            message_type,
            business_type: if proto.business_type.is_empty() { None } else { Some(proto.business_type.clone()) },
            receiver_id: if proto.receiver_id.is_empty() { None } else { Some(proto.receiver_id.clone()) },
            channel_id: None,
            content,
            content_type,
            attachments: Self::convert_attachments(&proto.attachments)?,
            quote: Self::convert_quote(&proto.quote)?,
            extra: proto.extra.clone(),
            attributes: proto.attributes.clone(),
            state,
            is_recalled,
            recalled_at: proto.recalled_at.as_ref()
                .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)),
            recall_reason: proto.recall_reason.clone().filter(|s| !s.is_empty()),
            is_burn_after_read: proto.is_burn_after_read,
            burn_after_seconds: if proto.burn_after_seconds > 0 { Some(proto.burn_after_seconds) } else { None },
            timeline: timeline.clone(),
            visibility: HashMap::new(),
            read_by,
            reactions,
            edit_history,
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
        // 转换消息状态（使用枚举辅助函数）
        let status = message_state::to_proto(msg.state);
        
        // 转换消息来源（使用枚举辅助函数）
        let source = message_source::to_proto(msg.source);
        
        // 转换会话类型（使用枚举辅助函数）
        let conversation_type = conversation_type::to_proto(msg.conversation_type);
        
        // 转换消息类型（使用枚举辅助函数）
        let message_type = message_type::to_proto(msg.message_type);
        
        // 转换内容类型（使用枚举辅助函数）
        let content_type = content_type::to_proto(msg.content_type);
        
        let content = flare_proto::decode_message_content(msg.content.as_slice())
            .map_err(|e| anyhow::anyhow!("Failed to decode MessageContent: {}", e))?;
        
        let created_at = Some(prost_types::Timestamp {
            seconds: msg.created_at.timestamp(),
            nanos: msg.created_at.timestamp_subsec_nanos() as i32,
        });
        
        // 审计上下文
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
            server_id: msg.server_id.clone().unwrap_or_default(),
            conversation_id: msg.conversation_id.clone().unwrap_or_default(),
            client_msg_id: msg.client_msg_id.clone(),
            sender_id: msg.sender_id.clone(),
            source,
            seq: msg.seq.unwrap_or(0),
            created_at,
            conversation_type,
            message_type,
            business_type: msg.business_type.clone().unwrap_or_default(),
            receiver_id: msg.receiver_id.clone().unwrap_or_default(),
            content: Some(content),
            content_type,
            attachments: Self::convert_attachments_to_proto(&msg.attachments)?,
            quote: Self::convert_quote_to_proto(&msg.quote)?,
            extra: msg.extra.clone(),
            attributes: msg.attributes.clone(),
            status,
            recalled_at: msg.recalled_at.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: dt.timestamp_subsec_nanos() as i32,
            }),
            recall_reason: msg.recall_reason.clone(),
            is_burn_after_read: msg.is_burn_after_read,
            burn_after_seconds: msg.burn_after_seconds.unwrap_or(0),
            current_edit_version: Some(msg.edit_history.len() as i32),
            last_edited_at: msg.edit_history.last().map(|e| prost_types::Timestamp {
                seconds: e.edited_at.timestamp(),
                nanos: e.edited_at.timestamp_subsec_nanos() as i32,
            }),
            audit,
            tags: msg.tags.clone(),
            offline_push_info: Self::convert_offline_push_info_to_proto(&msg.offline_push_info),
            extensions: Vec::new(),
            tenant: msg.extra.get("tenant").cloned().unwrap_or_default(),
            ..Default::default()
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
    
    /// 转换引用内容（从 Proto 到 Domain）
    fn convert_quote(
        proto_quote: &Option<flare_proto::common::QuoteContent>,
    ) -> Result<Option<crate::domain::message::QuoteContent>> {
        match proto_quote {
            Some(proto) => Ok(Some(crate::domain::message::QuoteContent {
                quoted_message_id: proto.quoted_message_id.clone(),
                quoted_sender_id: proto.quoted_sender_id.clone(),
                quoted_text_preview: proto.quoted_text_preview.clone(),
                quoted_content: proto.quoted_content.as_ref()
                    .map(|c| c.encode_to_bytes())
                    .transpose()
                    .map_err(|e| anyhow::anyhow!("Failed to encode quoted MessageContent: {}", e))?,
            })),
            None => Ok(None),
        }
    }
    
    /// 转换引用内容（从 Domain 到 Proto）
    fn convert_quote_to_proto(
        quote: &Option<crate::domain::message::QuoteContent>,
    ) -> Result<Option<flare_proto::common::QuoteContent>> {
        match quote {
            Some(q) => {
                let quoted_content = q.quoted_content.as_ref().map(|bytes| {
                    flare_proto::decode_message_content(bytes.as_slice())
                        .map_err(|e| anyhow::anyhow!("Failed to decode quoted_content: {}", e))
                }).transpose()?;
                
                Ok(Some(flare_proto::common::QuoteContent {
                    quoted_message_id: q.quoted_message_id.clone(),
                    quoted_sender_id: q.quoted_sender_id.clone(),
                    quoted_text_preview: q.quoted_text_preview.clone(),
                    quoted_content,
                }))
            },
            None => Ok(None),
        }
    }
    
    /// 转换可见性（从 Proto 到 Domain）；Message 瘦身后按需接口使用
    #[allow(dead_code)]
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
    
    /// 转换可见性（从 Domain 到 Proto）；Message 瘦身后按需接口使用
    #[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::message::MessageState;

    #[test]
    fn test_from_proto_should_fix_created_state_for_synced_messages() {
        // 构造一个 Proto 消息，模拟从服务端同步下来的消息
        // status = 1 (Created), server_id = "server-123"
        let proto_msg = flare_proto::flare::common::v1::Message {
            server_id: "server-123".to_string(),
            client_msg_id: "client-123".to_string(),
            conversation_id: "conv-123".to_string(),
            sender_id: "user-1".to_string(),
            status: 1, // Created
            ..Default::default()
        };

        // 执行转换
        let result = MessageConverter::from_proto(&proto_msg);

        // 验证结果
        assert!(result.is_ok());
        let msg = result.unwrap();
        
        // 验证状态是否被修正为 Sent
        assert_eq!(msg.state, MessageState::Sent, "Message state should be corrected to Sent for synced messages");
        assert_eq!(msg.server_id, Some("server-123".to_string()));
    }

    #[test]
    fn test_from_proto_should_keep_created_state_if_no_server_id() {
        // 构造一个 Proto 消息，模拟本地发送并未同步的消息（虽然这种情况很少通过 from_proto 转换）
        // status = 1 (Created), server_id = ""
        let proto_msg = flare_proto::flare::common::v1::Message {
            server_id: "".to_string(),
            client_msg_id: "client-123".to_string(),
            conversation_id: "conv-123".to_string(),
            sender_id: "user-1".to_string(),
            status: 1, // Created
            ..Default::default()
        };

        // 执行转换
        let result = MessageConverter::from_proto(&proto_msg);

        // 验证结果
        assert!(result.is_ok());
        let msg = result.unwrap();
        
        // 验证状态保持为 Created
        assert_eq!(msg.state, MessageState::Created, "Message state should remain Created if no server_id");
    }
}
