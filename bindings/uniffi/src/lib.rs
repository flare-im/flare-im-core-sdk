//! Flare IM Core SDK - UniFFI Bindings
//!
//! 提供跨语言绑定（Kotlin、Swift）
//! 自动从 UDL 文件生成各语言绑定

#![allow(clippy::too_many_arguments)]

use uniffi::Record;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::runtime::Runtime;

// 生成 UniFFI 绑定代码
uniffi::include_scaffolding!("im");

// ============================================================================
// 错误类型
// ============================================================================

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SdkError {
    #[error("配置错误: {0}")]
    Config(String),
    
    #[error("连接错误: {0}")]
    Connection(String),
    
    #[error("认证错误: {0}")]
    Authentication(String),
    
    #[error("消息错误: {0}")]
    Message(String),
    
    #[error("内部错误: {0}")]
    Internal(String),
}

// ============================================================================
// 配置类型
// ============================================================================

#[derive(Debug, Clone, uniffi::Record)]
pub struct SdkConfig {
    pub websocket_url: Option<String>,
    pub quic_url: Option<String>,
    pub storage_path: Option<String>,
    pub media_cache_path: Option<String>,
    #[uniffi(default = "info")]
    pub log_level: String,
}

impl From<SdkConfig> for flare_im_core_sdk::config::SdkConfig {
    fn from(config: SdkConfig) -> Self {
        use flare_im_core_sdk::config::SdkConfigBuilder;
        
        let mut builder = flare_im_core_sdk::config::SdkConfig::builder();
        
        if let Some(ws_url) = config.websocket_url {
            builder = builder.websocket_url(ws_url);
        }
        
        if let Some(quic_url) = config.quic_url {
            builder = builder.quic_url(quic_url);
        }
        
        if let Some(storage_path) = config.storage_path {
            builder = builder.storage_path(storage_path);
        }
        
        if let Some(media_cache_path) = config.media_cache_path {
            builder = builder.media_cache_path(media_cache_path);
        }
        
        builder = builder.log_level(&config.log_level);
        
        builder.build()
    }
}

// ============================================================================
// 消息类型
// ============================================================================

#[derive(Debug, Clone, uniffi::Record)]
pub struct Message {
    pub server_id: String,
    pub conversation_id: String,
    pub client_msg_id: String,
    pub sender_id: String,
    pub seq: Option<u64>,
    pub timestamp: String,  // ISO 8601 格式
    pub conversation_type: String,
    pub message_type: String,
    pub receiver_id: Option<String>,
    pub content_json: String,  // JSON 格式的内容
    pub content_type: String,
    pub state: String,
    pub version: u64,
}

impl From<flare_im_core_sdk::domain::message::Message> for Message {
    fn from(msg: flare_im_core_sdk::domain::message::Message) -> Self {
        // 将 MessageContent 解码并转换为 JSON
        let content_json = if let Ok(content) = flare_proto::decode_message_content(&msg.content) {
            // 提取文本内容
            if let Some(flare_proto::flare::common::v1::message_content::Content::Text(text_content)) = content.content {
                serde_json::json!({
                    "text": text_content.text,
                    "mentions": text_content.mentions
                }).to_string()
            } else {
                // 其他类型的内容，序列化为 JSON
                serde_json::to_string(&content).unwrap_or_else(|_| "{}".to_string())
            }
        } else {
            "{}".to_string()
        };
        
        Message {
            server_id: msg.server_id,
            conversation_id: msg.conversation_id,
            client_msg_id: msg.client_msg_id,
            sender_id: msg.sender_id,
            seq: msg.seq,
            timestamp: msg.timestamp.to_rfc3339(),
            conversation_type: format!("{:?}", msg.conversation_type),
            message_type: format!("{:?}", msg.message_type),
            receiver_id: msg.receiver_id,
            content_json,
            content_type: format!("{:?}", msg.content_type),
            state: format!("{:?}", msg.state),
            version: msg.version,
        }
    }
}

impl TryFrom<Message> for flare_im_core_sdk::domain::message::Message {
    type Error = SdkError;
    
    fn try_from(msg: Message) -> Result<Self, Self::Error> {
        // 解析时间戳
        let timestamp = chrono::DateTime::parse_from_rfc3339(&msg.timestamp)
            .map_err(|e| SdkError::Message(format!("Invalid timestamp: {}", e)))?
            .with_timezone(&chrono::Utc);
        
        // 解析枚举类型
        let conversation_type = parse_conversation_type(&msg.conversation_type)?;
        let message_type = parse_message_type(&msg.message_type)?;
        let content_type = parse_content_type(&msg.content_type)?;
        let state = parse_message_state(&msg.state)?;
        
        // 解析内容 JSON 并编码为 bytes
        // 注意：这里简化处理，实际应该根据 message_type 构建正确的 MessageContent
        // 对于文本消息，直接使用 content_json 作为文本内容
        let content_bytes = if msg.message_type == "Text" {
            // 对于文本消息，尝试从 JSON 中提取 text 字段
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&msg.content_json) {
                if let Some(text) = json_value.get("text").and_then(|v| v.as_str()) {
                    // 构建 TextContent 并编码
                    use flare_proto::flare::common::v1::{MessageContent, message_content::Content, TextContent};
                    let mut proto_content = MessageContent::default();
                    proto_content.content = Some(Content::Text(TextContent {
                        text: text.to_string(),
                        mentions: vec![],
                    }));
                    proto_content.encode_to_bytes()
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        
        Ok(flare_im_core_sdk::domain::message::Message {
            server_id: msg.server_id,
            conversation_id: msg.conversation_id,
            client_msg_id: msg.client_msg_id,
            sender_id: msg.sender_id,
            source: flare_im_core_sdk::domain::message::MessageSource::User,
            seq: msg.seq,
            timestamp,
            conversation_type,
            message_type,
            business_type: None,
            receiver_id: msg.receiver_id,
            channel_id: None,
            content: content_bytes,
            content_type,
            attachments: vec![],
            quote: None,
            extra: std::collections::HashMap::new(),
            attributes: std::collections::HashMap::new(),
            state,
            is_recalled: false,
            recalled_at: None,
            recall_reason: None,
            is_burn_after_read: false,
            burn_after_seconds: None,
            timeline: flare_im_core_sdk::domain::message::MessageTimeline::default(),
            visibility: std::collections::HashMap::new(),
            read_by: vec![],
            reactions: vec![],
            edit_history: vec![],
            tenant: flare_im_core_sdk::domain::message::TenantContext {
                tenant_id: String::new(),
                user_id: String::new(),
            },
            audit: None,
            tags: vec![],
            offline_push_info: None,
            version: msg.version,
            created_at: timestamp,
            updated_at: timestamp,
        })
    }
}

// 辅助函数：解析枚举类型
fn parse_conversation_type(s: &str) -> Result<flare_im_core_sdk::domain::message::ConversationType, SdkError> {
    match s {
        "Single" => Ok(flare_im_core_sdk::domain::message::ConversationType::Single),
        "Group" => Ok(flare_im_core_sdk::domain::message::ConversationType::Group),
        _ => Err(SdkError::Message {
            message: format!("Unknown conversation type: {}", s),
        }),
    }
}

fn parse_message_type(s: &str) -> Result<flare_im_core_sdk::domain::message::MessageType, SdkError> {
    match s {
        "Text" => Ok(flare_im_core_sdk::domain::message::MessageType::Text),
        "Image" => Ok(flare_im_core_sdk::domain::message::MessageType::Image),
        "Audio" => Ok(flare_im_core_sdk::domain::message::MessageType::Audio),
        "Video" => Ok(flare_im_core_sdk::domain::message::MessageType::Video),
        "File" => Ok(flare_im_core_sdk::domain::message::MessageType::File),
        _ => Err(SdkError::Message {
            message: format!("Unknown message type: {}", s),
        }),
    }
}

fn parse_content_type(s: &str) -> Result<flare_im_core_sdk::domain::message::ContentType, SdkError> {
    match s {
        "Text" => Ok(flare_im_core_sdk::domain::message::ContentType::Text),
        "Image" => Ok(flare_im_core_sdk::domain::message::ContentType::Image),
        "Audio" => Ok(flare_im_core_sdk::domain::message::ContentType::Audio),
        "Video" => Ok(flare_im_core_sdk::domain::message::ContentType::Video),
        "File" => Ok(flare_im_core_sdk::domain::message::ContentType::File),
        _ => Err(SdkError::Message {
            message: format!("Unknown content type: {}", s),
        }),
    }
}

fn parse_message_state(s: &str) -> Result<flare_im_core_sdk::domain::message::MessageState, SdkError> {
    match s {
        "Created" => Ok(flare_im_core_sdk::domain::message::MessageState::Created),
        "Sent" => Ok(flare_im_core_sdk::domain::message::MessageState::Sent),
        "Delivered" => Ok(flare_im_core_sdk::domain::message::MessageState::Delivered),
        "Read" => Ok(flare_im_core_sdk::domain::message::MessageState::Read),
        "Failed" => Ok(flare_im_core_sdk::domain::message::MessageState::Failed),
        "Recalled" => Ok(flare_im_core_sdk::domain::message::MessageState::Recalled),
        _ => Err(SdkError::Message {
            message: format!("Unknown message state: {}", s),
        }),
    }
}

// ============================================================================
// 租户上下文
// ============================================================================

#[derive(Debug, Clone, uniffi::Record)]
pub struct TenantContext {
    pub tenant_id: String,
    pub user_id: String,
}

impl From<TenantContext> for flare_im_core_sdk::domain::message::TenantContext {
    fn from(ctx: TenantContext) -> Self {
        flare_im_core_sdk::domain::message::TenantContext {
            tenant_id: ctx.tenant_id,
            user_id: ctx.user_id,
        }
    }
}

// ============================================================================
// 会话类型
// ============================================================================

#[derive(Debug, Clone, uniffi::Record)]
pub struct Conversation {
    pub conversation_id: String,
    pub conversation_type: String,
    pub business_type: Option<String>,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub unread_count: u32,
    pub max_seq: u64,
    pub last_read_seq: u64,
    pub is_muted: bool,
    pub is_pinned: bool,
    pub last_message_preview: Option<String>,
}

impl From<flare_im_core_sdk::domain::conversation::Conversation> for Conversation {
    fn from(conv: flare_im_core_sdk::domain::conversation::Conversation) -> Self {
        Conversation {
            conversation_id: conv.conversation_id,
            conversation_type: conv.conversation_type,
            business_type: conv.business_type,
            display_name: conv.display_name,
            avatar_url: conv.avatar_url,
            unread_count: conv.unread_count,
            max_seq: conv.max_seq,
            last_read_seq: conv.last_read_seq,
            is_muted: conv.is_muted,
            is_pinned: conv.is_pinned,
            last_message_preview: conv.last_message
                .as_ref()
                .and_then(|m| m.content_preview.clone()),
        }
    }
}

// ============================================================================
// SDK 主接口（同步版本，内部使用异步运行时）
// ============================================================================

#[derive(uniffi::Object)]
pub struct ImCoreSdk {
    sdk: Arc<flare_im_core_sdk::interface::facade::ImCoreSdk>,
    runtime: Arc<Runtime>,
}

#[uniffi::export]
impl ImCoreSdk {
    #[uniffi::constructor]
    pub fn new(config: SdkConfig) -> Result<Arc<Self>, SdkError> {
        let runtime = Arc::new(
            Runtime::new()
                .map_err(|e| SdkError::Internal {
                    message: format!("Failed to create runtime: {}", e),
                })?
        );
        
        let config: flare_im_core_sdk::config::SdkConfig = config.into();
        
        let sdk = runtime.block_on(
            flare_im_core_sdk::interface::facade::ImCoreSdk::new(config)
        )
        .map_err(|e| SdkError::Config(e.to_string()))?;
        
        Ok(Arc::new(Self {
            sdk: Arc::new(sdk),
            runtime,
        }))
    }
    
    // ========== 生命周期管理（异步回调） ==========
    
    pub fn login(&self, user_id: String, token: String, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.login(user_id, token).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Authentication {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    pub fn logout(&self, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.logout().await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    pub fn connect(&self, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.connect().await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Connection {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    pub fn bootstrap_sync(&self, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.bootstrap_sync().await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    // ========== 消息操作（异步回调） ==========
    
    /// 创建并发送文本消息（推荐：一步完成）
    pub fn create_and_send_text_message(
        &self,
        conversation_id: String,
        sender_id: String,
        text: String,
        tenant: TenantContext,
        receiver_id: Option<String>,
        callback: Arc<dyn MessageOperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        let tenant_domain: flare_im_core_sdk::domain::message::TenantContext = tenant.into();
        
        runtime.spawn(async move {
            // 1. 创建消息
            let message = match sdk.message().create_text_message(
                conversation_id,
                sender_id,
                text,
                tenant_domain,
                receiver_id,
            ) {
                Ok(msg) => msg,
                Err(e) => {
                    callback.on_error(SdkError::Message {
                        message: format!("Failed to create message: {}", e),
                    });
                    return;
                }
            };
            
            // 2. 发送消息
            match sdk.message().send_message(message.clone()).await {
                Ok(_) => {
                    let uniffi_message: Message = message.into();
                    callback.on_success(uniffi_message);
                }
                Err(e) => {
                    callback.on_error(SdkError::Message {
                        message: format!("Failed to send message: {}", e),
                    });
                }
            }
        });
    }
    
    /// 发送消息
    pub fn send_message(&self, message: Message, callback: Arc<dyn MessageOperationCallback>) {
        let domain_message_result: Result<flare_im_core_sdk::domain::message::Message, SdkError> = message.try_into();
        
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        match domain_message_result {
            Ok(domain_message) => {
                // 在后台任务中执行异步操作
                runtime.spawn(async move {
                    match sdk.message().send_message(domain_message).await {
                        Ok(_) => {
                            // 发送成功，但需要获取更新后的消息（带 server_id 等）
                            // 这里简化处理，返回原始消息
                            // TODO: 从事件总线获取发送后的完整消息
                            callback.on_success(message);
                        }
                        Err(e) => {
                            callback.on_error(SdkError::Message {
                                message: e.to_string(),
                            });
                        }
                    }
                });
            }
            Err(e) => {
                // 转换失败，立即回调错误
                callback.on_error(e);
            }
        }
    }
    
    
    /// 查询消息（本地查询，仍使用回调保持一致性）
    pub fn get_messages(
        &self,
        conversation_id: String,
        limit: Option<u32>,
        callback: Arc<dyn MessageListCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            use flare_im_core_sdk::application::queries::ListMessagesQuery;
            
            match sdk.sdk_context().query_handler.list_messages(ListMessagesQuery {
                conversation_id,
                limit: limit.map(|l| l as usize),
                cursor: None,
            }).await {
                Ok(messages) => {
                    let uniffi_messages: Vec<Message> = messages.into_iter().map(Message::from).collect();
                    callback.on_success(uniffi_messages);
                }
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 搜索消息（本地查询）
    pub fn search_messages(
        &self,
        conversation_id: String,
        keyword: String,
        limit: Option<u32>,
        callback: Arc<dyn MessageListCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.message().search_local_messages(
                conversation_id,
                keyword,
                limit.map(|l| l as usize),
            ).await {
                Ok(messages) => {
                    let uniffi_messages: Vec<Message> = messages.into_iter().map(Message::from).collect();
                    callback.on_success(uniffi_messages);
                }
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    // ========== 消息操作（异步回调） ==========
    
    /// 撤回消息
    pub fn revoke_message(
        &self,
        client_msg_id: String,
        recaller_id: String,
        callback: Arc<dyn MessageOperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.message().revoke_message(client_msg_id.clone(), recaller_id.clone(), None).await {
                Ok(_) => {
                    // 撤回成功，返回一个占位消息
                    // TODO: 从事件总线获取被撤回的完整消息
                    let placeholder_msg = Message {
                        server_id: String::new(),
                        conversation_id: String::new(),
                        client_msg_id: client_msg_id.clone(),
                        sender_id: recaller_id.clone(),
                        seq: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        conversation_type: String::new(),
                        message_type: "Text".to_string(),
                        receiver_id: None,
                        content_json: "{}".to_string(),
                        content_type: "Text".to_string(),
                        state: "Recalled".to_string(),
                        version: 0,
                    };
                    callback.on_success(placeholder_msg);
                }
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 编辑消息
    pub fn edit_message(
        &self,
        client_msg_id: String,
        editor_id: String,
        new_content: String,
        callback: Arc<dyn MessageOperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        let content_bytes = new_content.as_bytes().to_vec();
        
        runtime.spawn(async move {
            match sdk.message().edit_message(client_msg_id.clone(), editor_id.clone(), content_bytes, None).await {
                Ok(_) => {
                    // 编辑成功，返回占位消息（TODO: 从事件总线获取编辑后的完整消息）
                    let placeholder_msg = Message {
                        server_id: String::new(),
                        conversation_id: String::new(),
                        client_msg_id: client_msg_id.clone(),
                        sender_id: editor_id.clone(),
                        seq: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        conversation_type: String::new(),
                        message_type: "Text".to_string(),
                        receiver_id: None,
                        content_json: format!(r#"{{"text": "{}"}}"#, new_content),
                        content_type: "Text".to_string(),
                        state: "Created".to_string(),
                        version: 0,
                    };
                    callback.on_success(placeholder_msg);
                }
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 删除消息
    pub fn delete_message(
        &self,
        client_msg_id: String,
        operator_id: String,
        callback: Arc<dyn OperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            use flare_im_core_sdk::domain::message::DeleteType;
            match sdk.message().delete_message(client_msg_id, operator_id, DeleteType::Soft, None).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 回复消息
    pub fn reply_message(
        &self,
        conversation_id: String,
        replied_msg_id: String,
        new_message: Message,
        callback: Arc<dyn MessageOperationCallback>,
    ) {
        let domain_message_result: Result<flare_im_core_sdk::domain::message::Message, SdkError> = new_message.clone().try_into();
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        match domain_message_result {
            Ok(domain_message) => {
                let message_clone = new_message.clone();
                runtime.spawn(async move {
                    match sdk.message().reply_message(conversation_id, replied_msg_id, domain_message).await {
                        Ok(_) => {
                            // 回复成功，返回消息
                            callback.on_success(message_clone);
                        }
                        Err(e) => callback.on_error(SdkError::Message {
                            message: e.to_string(),
                        }),
                    }
                });
            }
            Err(e) => callback.on_error(e),
        }
    }
    
    /// 转发消息
    pub fn forward_messages(
        &self,
        conversation_ids: Vec<String>,
        messages: Vec<Message>,
        callback: Arc<dyn OperationCallback>,
    ) {
        let domain_messages_result: Result<Vec<_>, _> = messages.iter().map(|m| m.clone().try_into()).collect();
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        match domain_messages_result {
            Ok(domain_messages) => {
                runtime.spawn(async move {
                    match sdk.message().forward_messages(conversation_ids, domain_messages).await {
                        Ok(_) => callback.on_success(),
                        Err(e) => callback.on_error(SdkError::Message {
                            message: e.to_string(),
                        }),
                    }
                });
            }
            Err(e) => callback.on_error(e),
        }
    }
    
    /// 添加反应
    pub fn add_reaction(
        &self,
        conversation_id: String,
        message_id: String,
        user_id: String,
        emoji: String,
        callback: Arc<dyn OperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.message().add_reaction(conversation_id, message_id, user_id, emoji).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 移除反应
    pub fn remove_reaction(
        &self,
        conversation_id: String,
        message_id: String,
        user_id: String,
        emoji: String,
        callback: Arc<dyn OperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.message().remove_reaction(conversation_id, message_id, user_id, emoji).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 标记消息已读
    pub fn mark_message_read(
        &self,
        conversation_id: String,
        message_ids: Vec<String>,
        user_id: String,
        callback: Arc<dyn OperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.message().batch_mark_message_read(conversation_id, message_ids, user_id).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Message {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    // ========== 会话操作（异步回调） ==========
    
    /// 获取会话列表（本地查询）
    pub fn get_all_conversations(&self, callback: Arc<dyn ConversationListCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().get_all_conversation_list().await {
                Ok(conversations) => {
                    let uniffi_conversations: Vec<Conversation> = conversations.into_iter().map(Conversation::from).collect();
                    callback.on_success(uniffi_conversations);
                }
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 分页获取会话列表（本地查询）
    pub fn get_conversations_paginated(
        &self,
        page: u32,
        page_size: u32,
        callback: Arc<dyn ConversationListCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().get_conversation_list_split(page as usize, page_size as usize).await {
                Ok((conversations, _total)) => {
                    let uniffi_conversations: Vec<Conversation> = conversations.into_iter().map(Conversation::from).collect();
                    callback.on_success(uniffi_conversations);
                }
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 获取单个会话（本地查询）
    pub fn get_conversation(&self, conversation_id: String, callback: Arc<dyn ConversationOperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().get_one_conversation(conversation_id).await {
                Ok(conversation) => {
                    let uniffi_conversation: Conversation = conversation.into();
                    callback.on_success(uniffi_conversation);
                }
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 获取多个会话（本地查询）
    pub fn get_multiple_conversations(
        &self,
        conversation_ids: Vec<String>,
        callback: Arc<dyn ConversationListCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().get_multiple_conversation(conversation_ids).await {
                Ok(conversations) => {
                    let uniffi_conversations: Vec<Conversation> = conversations.into_iter().map(Conversation::from).collect();
                    callback.on_success(uniffi_conversations);
                }
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 标记会话已读
    pub fn mark_conversation_read(&self, conversation_id: String, user_id: String, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().mark_conversation_message_as_read(conversation_id, user_id).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 设置会话草稿
    pub fn set_conversation_draft(&self, conversation_id: String, draft: Option<String>, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().set_conversation_draft(conversation_id, draft).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 隐藏会话
    pub fn hide_conversation(&self, conversation_id: String, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().hide_conversation(conversation_id).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 删除会话
    pub fn delete_conversation(&self, conversation_id: String, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().delete_conversation_and_delete_all_msg(conversation_id).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 清空会话消息
    pub fn clear_conversation_messages(&self, conversation_id: String, callback: Arc<dyn OperationCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().clear_conversation_and_delete_all_msg(conversation_id).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 设置会话信息
    pub fn set_conversation_info(
        &self,
        conversation_id: String,
        display_name: Option<String>,
        avatar_url: Option<String>,
        description: Option<String>,
        callback: Arc<dyn OperationCallback>,
    ) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().set_conversation(
                conversation_id,
                display_name,
                avatar_url,
                description,
                None, // announcement
            ).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 设置输入状态
    pub fn set_input_state(
        &self,
        conversation_id: String,
        user_id: String,
        state_type: String,
        callback: Arc<dyn OperationCallback>,
    ) {
        use flare_im_core_sdk::domain::conversation::InputStateType;
        let state = match state_type.as_str() {
            "Typing" => InputStateType::Typing,
            "Stopped" => InputStateType::Stopped,
            _ => {
                callback.on_error(SdkError::Internal {
                    message: format!("Unknown input state type: {}", state_type),
                });
                return;
            }
        };
        
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().change_input_states(conversation_id, user_id, state).await {
                Ok(_) => callback.on_success(),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 获取总未读数（本地查询）
    pub fn get_total_unread_count(&self, callback: Arc<dyn UnreadCountCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().get_total_unread_msg_count().await {
                Ok(count) => callback.on_success(count),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    /// 获取输入状态（本地查询）
    pub fn get_input_states(&self, conversation_id: String, callback: Arc<dyn InputStatesCallback>) {
        let sdk = self.sdk.clone();
        let runtime = self.runtime.clone();
        
        runtime.spawn(async move {
            match sdk.conversation().get_input_states(conversation_id).await {
                Ok(Some(states_json)) => callback.on_success(Some(states_json.to_string())),
                Ok(None) => callback.on_success(None),
                Err(e) => callback.on_error(SdkError::Internal {
                    message: e.to_string(),
                }),
            }
        });
    }
    
    // ========== 事件订阅 ==========
    
    pub fn subscribe_message_events(&self, callback: Arc<dyn MessageEventSubscriber>) -> Result<String, SdkError> {
        use flare_im_core_sdk::domain::event::subscribers::MessageEventSubscriber as DomainMessageEventSubscriber;
        
        // 创建包装器，将 UniFFI 回调转换为领域订阅者
        let wrapper = MessageEventSubscriberWrapper { callback };
        let subscriber_id = self.runtime.block_on(
            self.sdk.events().subscribe_message(Arc::new(wrapper))
        );
        
        Ok(subscriber_id)
    }
    
    pub fn unsubscribe_message_events(&self, subscriber_id: String) -> Result<(), SdkError> {
        let _removed = self.runtime.block_on(
            self.sdk.events().unsubscribe_message(&subscriber_id)
        );
        Ok(())
    }
    
    pub fn subscribe_connection_events(&self, callback: Arc<dyn ConnectionEventSubscriber>) -> Result<String, SdkError> {
        use flare_im_core_sdk::domain::event::subscribers::ConnectionEventSubscriber as DomainConnectionEventSubscriber;
        
        let wrapper = ConnectionEventSubscriberWrapper { callback };
        let subscriber_id = self.runtime.block_on(
            self.sdk.events().subscribe_connection(Arc::new(wrapper))
        );
        
        Ok(subscriber_id)
    }
    
    pub fn unsubscribe_connection_events(&self, subscriber_id: String) -> Result<(), SdkError> {
        let _removed = self.runtime.block_on(
            self.sdk.events().unsubscribe_connection(&subscriber_id)
        );
        Ok(())
    }
    
    pub fn subscribe_conversation_events(&self, callback: Arc<dyn ConversationEventSubscriber>) -> Result<String, SdkError> {
        use flare_im_core_sdk::domain::event::subscribers::ConversationEventSubscriber as DomainConversationEventSubscriber;
        
        let wrapper = ConversationEventSubscriberWrapper { callback };
        let subscriber_id = self.runtime.block_on(
            self.sdk.events().subscribe_conversation(Arc::new(wrapper))
        );
        
        Ok(subscriber_id)
    }
    
    pub fn unsubscribe_conversation_events(&self, subscriber_id: String) -> Result<(), SdkError> {
        let _removed = self.runtime.block_on(
            self.sdk.events().unsubscribe_conversation(&subscriber_id)
        );
        Ok(())
    }
}

// ============================================================================
// 事件订阅者包装器（将 UniFFI 回调转换为领域订阅者）
// ============================================================================

use flare_im_core_sdk::domain::event::subscribers::MessageEventSubscriber as DomainMessageEventSubscriber;
use flare_im_core_sdk::domain::event::{
    MessageCreated, MessageSent, MessageSendFailed, MessageDelivered, MessageRead,
    MessageRecalled, MessageEdited, MessageDeleted, MessageReactionAdded, MessageReactionRemoved,
};

// ============================================================================
// 操作回调接口（用于异步操作）
// ============================================================================

#[uniffi::export]
pub trait OperationCallback: Send + Sync {
    fn on_success(&self);
    fn on_error(&self, error: SdkError);
}

#[uniffi::export]
pub trait MessageOperationCallback: Send + Sync {
    fn on_success(&self, message: Message);
    fn on_error(&self, error: SdkError);
}

#[uniffi::export]
pub trait ConversationOperationCallback: Send + Sync {
    fn on_success(&self, conversation: Conversation);
    fn on_error(&self, error: SdkError);
}

#[uniffi::export]
pub trait MessageListCallback: Send + Sync {
    fn on_success(&self, messages: Vec<Message>);
    fn on_error(&self, error: SdkError);
}

#[uniffi::export]
pub trait ConversationListCallback: Send + Sync {
    fn on_success(&self, conversations: Vec<Conversation>);
    fn on_error(&self, error: SdkError);
}

#[uniffi::export]
pub trait UnreadCountCallback: Send + Sync {
    fn on_success(&self, unread_count: u32);
    fn on_error(&self, error: SdkError);
}

#[uniffi::export]
pub trait InputStatesCallback: Send + Sync {
    fn on_success(&self, states_json: Option<String>);
    fn on_error(&self, error: SdkError);
}

// ============================================================================
// 事件订阅者接口
// ============================================================================

#[uniffi::export]
pub trait MessageEventSubscriber: Send + Sync {
    fn on_message_created(&self, message: Message);
    fn on_message_sent(&self, message: Message);
    fn on_message_send_failed(&self, message_id: String, error: String);
    fn on_message_delivered(&self, conversation_id: String, message_id: String, user_id: String);
    fn on_message_read(&self, conversation_id: String, message_id: String, user_id: String);
    fn on_message_recalled(&self, conversation_id: String, message_id: String, user_id: String);
    fn on_message_edited(&self, conversation_id: String, message_id: String, new_message: Message);
    fn on_message_deleted(&self, conversation_id: String, message_id: String, user_id: String);
    fn on_reaction_added(&self, conversation_id: String, message_id: String, user_id: String, emoji: String);
    fn on_reaction_removed(&self, conversation_id: String, message_id: String, user_id: String, emoji: String);
}

struct MessageEventSubscriberWrapper {
    callback: Arc<dyn MessageEventSubscriber>,
}

#[async_trait::async_trait]
impl DomainMessageEventSubscriber for MessageEventSubscriberWrapper {
    async fn on_message_created(&self, event: &MessageCreated) -> anyhow::Result<()> {
        // MessageCreated 只有 message_id, conversation_id, sender_id, content
        // 需要从 content 构建 Message，这里简化处理，创建一个最小化的 Message
        // 实际实现中应该从事件总线获取完整消息对象
        // 这里先用一个占位实现
        let _ = event;
        // TODO: 从事件总线获取完整消息
        Ok(())
    }
    
    async fn on_message_sent(&self, event: &MessageSent) -> anyhow::Result<()> {
        // MessageSent 只有 message_id 和 seq
        // 需要从其他地方获取完整消息对象
        let _ = event;
        // TODO: 从事件总线获取完整消息
        Ok(())
    }
    
    async fn on_message_send_failed(&self, event: &MessageSendFailed) -> anyhow::Result<()> {
        self.callback.on_message_send_failed(
            event.message_id.clone(),
            event.error.clone(),
        );
        Ok(())
    }
    
    async fn on_message_delivered(&self, event: &MessageDelivered) -> anyhow::Result<()> {
        // MessageDelivered 只有 message_id，需要从其他地方获取 conversation_id 和 user_id
        // 这里简化处理，使用空字符串
        self.callback.on_message_delivered(
            String::new(), // conversation_id
            event.message_id.clone(),
            String::new(), // user_id
        );
        Ok(())
    }
    
    async fn on_message_read(&self, event: &MessageRead) -> anyhow::Result<()> {
        // MessageRead 只有 message_id 和 reader_id
        self.callback.on_message_read(
            String::new(), // conversation_id
            event.message_id.clone(),
            event.reader_id.clone(),
        );
        Ok(())
    }
    
    async fn on_message_recalled(&self, event: &MessageRecalled) -> anyhow::Result<()> {
        self.callback.on_message_recalled(
            String::new(), // conversation_id
            event.message_id.clone(),
            event.recaller_id.clone(),
        );
        Ok(())
    }
    
    async fn on_message_edited(&self, event: &MessageEdited) -> anyhow::Result<()> {
        // MessageEdited 有 new_content，但需要构建完整 Message
        // 这里简化处理，创建一个最小化的 Message
        // TODO: 从 new_content 构建完整 Message
        let _msg = Message {
            server_id: String::new(),
            conversation_id: String::new(),
            client_msg_id: event.message_id.clone(),
            sender_id: event.editor_id.clone(),
            seq: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            conversation_type: String::new(),
            message_type: "Text".to_string(),
            receiver_id: None,
            content_json: serde_json::to_string(&event.new_content).unwrap_or_default(),
            content_type: "Text".to_string(),
            state: "Created".to_string(),
            version: 0,
        };
        // TODO: 使用完整消息对象
        Ok(())
    }
    
    async fn on_message_deleted(&self, event: &MessageDeleted) -> anyhow::Result<()> {
        self.callback.on_message_deleted(
            String::new(), // conversation_id
            event.message_id.clone(),
            event.operator_id.clone(),
        );
        Ok(())
    }
    
    async fn on_reaction_added(&self, event: &MessageReactionAdded) -> anyhow::Result<()> {
        self.callback.on_reaction_added(
            String::new(), // conversation_id
            event.message_id.clone(),
            event.user_id.clone(),
            event.emoji.clone(),
        );
        Ok(())
    }
    
    async fn on_reaction_removed(&self, event: &MessageReactionRemoved) -> anyhow::Result<()> {
        self.callback.on_reaction_removed(
            String::new(), // conversation_id
            event.message_id.clone(),
            event.user_id.clone(),
            event.emoji.clone(),
        );
        Ok(())
    }
}

#[uniffi::export]
pub trait ConnectionEventSubscriber: Send + Sync {
    fn on_connected(&self, connection_id: String);
    fn on_disconnected(&self, reason: String);
    fn on_reconnecting(&self, attempt: u32);
    fn on_reconnected(&self);
    fn on_connect_failed(&self, error: String);
}

struct ConnectionEventSubscriberWrapper {
    callback: Arc<dyn ConnectionEventSubscriber>,
}

use flare_im_core_sdk::domain::event::subscribers::ConnectionEventSubscriber as DomainConnectionEventSubscriber;
use flare_im_core_sdk::domain::event::{
    ConnectionConnected, ConnectionDisconnected, ConnectionReconnecting,
    ConnectionReconnected, ConnectionConnectFailed,
};

#[async_trait::async_trait]
impl DomainConnectionEventSubscriber for ConnectionEventSubscriberWrapper {
    async fn on_connected(&self, event: &ConnectionConnected) -> anyhow::Result<()> {
        self.callback.on_connected(event.connection_id.clone());
        Ok(())
    }
    
    async fn on_disconnected(&self, event: &ConnectionDisconnected) -> anyhow::Result<()> {
        self.callback.on_disconnected(event.reason.clone().unwrap_or_default());
        Ok(())
    }
    
    async fn on_reconnecting(&self, event: &ConnectionReconnecting) -> anyhow::Result<()> {
        self.callback.on_reconnecting(event.attempt);
        Ok(())
    }
    
    async fn on_reconnected(&self, _event: &ConnectionReconnected) -> anyhow::Result<()> {
        self.callback.on_reconnected();
        Ok(())
    }
    
    async fn on_connect_failed(&self, event: &ConnectionConnectFailed) -> anyhow::Result<()> {
        self.callback.on_connect_failed(event.error.clone().unwrap_or_default());
        Ok(())
    }
}

#[uniffi::export]
pub trait ConversationEventSubscriber: Send + Sync {
    fn on_conversation_created(&self, conversation: Conversation);
    fn on_unread_updated(&self, conversation_id: String, unread_count: u32);
    fn on_last_message_updated(&self, conversation_id: String, message: Option<Message>);
    fn on_marked_as_read(&self, conversation_id: String, user_id: String);
    fn on_draft_updated(&self, conversation_id: String, draft: Option<String>);
    fn on_hidden(&self, conversation_id: String);
    fn on_deleted(&self, conversation_id: String);
    fn on_messages_cleared(&self, conversation_id: String);
    fn on_updated(&self, conversation: Conversation);
    fn on_input_state_updated(&self, conversation_id: String, user_id: String, state_type: String);
}

struct ConversationEventSubscriberWrapper {
    callback: Arc<dyn ConversationEventSubscriber>,
}

use flare_im_core_sdk::domain::event::subscribers::ConversationEventSubscriber as DomainConversationEventSubscriber;
use flare_im_core_sdk::domain::event::{
    ConversationCreated, ConversationUnreadUpdated, ConversationLastMessageUpdated,
    ConversationMarkedAsRead, ConversationDraftUpdated, ConversationHidden,
    ConversationDeleted, ConversationMessagesCleared, ConversationUpdated,
    ConversationInputStateUpdated,
};

#[async_trait::async_trait]
impl DomainConversationEventSubscriber for ConversationEventSubscriberWrapper {
    async fn on_conversation_created(&self, event: &ConversationCreated) -> anyhow::Result<()> {
        let conv: Conversation = event.conversation.clone().into();
        self.callback.on_conversation_created(conv);
        Ok(())
    }
    
    async fn on_unread_updated(&self, event: &ConversationUnreadUpdated) -> anyhow::Result<()> {
        self.callback.on_unread_updated(
            event.conversation_id.clone(),
            event.unread_count,
        );
        Ok(())
    }
    
    async fn on_last_message_updated(&self, event: &ConversationLastMessageUpdated) -> anyhow::Result<()> {
        let msg = event.last_message.as_ref().map(|m| Message::from(m.clone()));
        self.callback.on_last_message_updated(
            event.conversation_id.clone(),
            msg,
        );
        Ok(())
    }
    
    async fn on_marked_as_read(&self, event: &ConversationMarkedAsRead) -> anyhow::Result<()> {
        self.callback.on_marked_as_read(
            event.conversation_id.clone(),
            event.user_id.clone(),
        );
        Ok(())
    }
    
    async fn on_draft_updated(&self, event: &ConversationDraftUpdated) -> anyhow::Result<()> {
        self.callback.on_draft_updated(
            event.conversation_id.clone(),
            event.draft.clone(),
        );
        Ok(())
    }
    
    async fn on_hidden(&self, event: &ConversationHidden) -> anyhow::Result<()> {
        self.callback.on_hidden(event.conversation_id.clone());
        Ok(())
    }
    
    async fn on_deleted(&self, event: &ConversationDeleted) -> anyhow::Result<()> {
        self.callback.on_deleted(event.conversation_id.clone());
        Ok(())
    }
    
    async fn on_messages_cleared(&self, event: &ConversationMessagesCleared) -> anyhow::Result<()> {
        self.callback.on_messages_cleared(event.conversation_id.clone());
        Ok(())
    }
    
    async fn on_updated(&self, event: &ConversationUpdated) -> anyhow::Result<()> {
        let conv: Conversation = event.conversation.clone().into();
        self.callback.on_updated(conv);
        Ok(())
    }
    
    async fn on_input_state_updated(&self, event: &ConversationInputStateUpdated) -> anyhow::Result<()> {
        self.callback.on_input_state_updated(
            event.conversation_id.clone(),
            event.user_id.clone(),
            format!("{:?}", event.state_type),
        );
        Ok(())
    }
}
