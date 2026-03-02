//! 网络消息分发器（应用层消息处理器）
//!
//! 负责订阅 NetworkMessage 通道，并将不同类型的消息分发给对应的处理器
//! 
//! 架构职责：
//! - 专门处理来自网络层的消息分发
//! - 不包含业务逻辑，只负责路由消息到适当的处理器
//! - 实现应用层与网络层的解耦

//! # 处理流程
//!
//! 1. 订阅 NetworkMessage 通道
//! 2. 根据消息类型分发到不同的处理器：
//!    - SyncMessages → SyncHandler
//!    - SyncConversations / ConversationSyncAll / ConversationDetail → ConversationSyncHandler
//!    - CustomPushData → CustomDataHandler
//!    - Received → 已由 MessageQueue 处理（不需要在这里处理）

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::infrastructure::network::NetworkMessage;
use crate::application::handlers::{
    SyncHandler,
    ConversationSyncHandler,
    CustomDataHandler,
};
use crate::domain::message_queue::MessageQueue;
use crate::domain::repository::ConversationRepository;
use crate::infrastructure::event_bus::EventBus;
use crate::application::extension::ExtensionRegistry;
use tracing::{info, warn, error, debug};

/// 网络消息分发器
pub struct NetworkMessageDispatcher {
    sync_handler: Arc<SyncHandler>,
    conversation_sync_handler: Arc<ConversationSyncHandler>,
    custom_data_handler: Arc<CustomDataHandler>,
}

impl NetworkMessageDispatcher {
    /// 创建新的网络消息分发器
    pub fn new(
        message_queue: Arc<MessageQueue>,
        conversation_repository: Arc<dyn ConversationRepository>,
        event_bus: Arc<EventBus>,
        extension_registry: Arc<ExtensionRegistry>,
    ) -> Self {
        let sync_handler = Arc::new(SyncHandler::new(
            message_queue.clone(),
            event_bus.clone(),
        ));
        
        let conversation_sync_handler = Arc::new(ConversationSyncHandler::new(
            conversation_repository.clone(),
            event_bus.clone(),
        ));
        
        let custom_data_handler = Arc::new(CustomDataHandler::new(
            event_bus.clone(),
            extension_registry,
        ));
        
        Self {
            sync_handler,
            conversation_sync_handler,
            custom_data_handler,
        }
    }
    
    /// 启动消息分发循环
    ///
    /// # 参数
    ///
    /// * `mut message_rx` - NetworkMessage 接收通道
    ///
    /// # 返回
    ///
    /// * `tokio::task::JoinHandle` - 后台任务句柄
    pub fn start(self, mut message_rx: mpsc::UnboundedReceiver<NetworkMessage>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("NetworkMessageDispatcher started");
            
            while let Some(network_msg) = message_rx.recv().await {
                // 性能优化：异步处理消息，不阻塞消息接收循环
                match network_msg {
                    // EventEnvelope 已在 NetworkClient 中由 EventStreamProcessor 处理，不应到达此处
                    NetworkMessage::EventEnvelope(_) => {
                        debug!("EventEnvelope already handled by EventStreamProcessor, skipping");
                    }
                    // 同步消息响应：由 SyncHandler 处理
                    NetworkMessage::SyncMessages(sync_resp) => {
                        let handler = self.sync_handler.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_sync_messages_response(sync_resp).await {
                                error!("Failed to handle SyncMessagesResponse: {}", e);
                            }
                        });
                    }
                    
                    // 会话增量同步响应：由 ConversationSyncHandler 处理
                    NetworkMessage::SyncConversations(resp) => {
                        let handler = self.conversation_sync_handler.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_sync_conversations_response(resp).await {
                                error!("Failed to handle SyncConversationsResponse: {}", e);
                            }
                        });
                    }
                    
                    // 全量会话同步响应：由 ConversationSyncHandler 处理
                    NetworkMessage::ConversationSyncAll(resp) => {
                        let handler = self.conversation_sync_handler.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_conversation_sync_all_response(resp).await {
                                error!("Failed to handle ConversationSyncAllResponse: {}", e);
                            }
                        });
                    }
                    
                    // 会话详情响应：由 ConversationSyncHandler 处理
                    NetworkMessage::ConversationDetail(resp) => {
                        let handler = self.conversation_sync_handler.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_conversation_detail_response(resp).await {
                                error!("Failed to handle GetConversationDetailResponse: {}", e);
                            }
                        });
                    }
                    
                    // 自定义推送数据：由 CustomDataHandler 处理
                    NetworkMessage::CustomPushData { data_type, payload, metadata } => {
                        let handler = self.custom_data_handler.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handler.handle_custom_push_data(
                                data_type,
                                payload,
                                metadata,
                            ).await {
                                error!("Failed to handle CustomPushData: {}", e);
                            }
                        });
                    }
                    
                    // 收到的消息：已由 MessageQueue 处理（NetworkClient::new_with_queue 中处理）
                    // 注意：Received 消息不应该到达这里，如果到达说明有逻辑问题
                    NetworkMessage::Received(_) => {
                        warn!("Received message reached dispatcher, should have been handled by MessageQueue");
                    }
                    
                    // 连接事件：记录日志，连接状态由 connection_rx 处理
                    NetworkMessage::Connected(connection_id) => {
                        info!(connection_id = %connection_id, "Network connected");
                    }
                    
                    NetworkMessage::Disconnected(reason) => {
                        warn!(reason = %reason, "Network disconnected");
                    }
                    
                    NetworkMessage::Error(err) => {
                        error!("Network error: {}", err);
                    }
                }
            }
            
            warn!("NetworkMessageDispatcher stopped (channel closed)");
        })
    }
}
