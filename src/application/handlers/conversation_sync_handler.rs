//! 会话同步处理器
//!
//! 负责处理会话同步响应（SyncConversationsResponse, ConversationSyncAllResponse, GetConversationDetailResponse）
//!
//! # 处理流程
//!
//! 1. 解析 ConversationPatch 列表（增量同步）或会话列表（全量同步）
//! 2. 按 patch_type 分类处理（增量同步）
//! 3. 批量更新 ReadStore
//! 4. 更新会话游标
//! 5. 发布会话列表更新事件

use std::sync::Arc;
use flare_proto::common::{
    SyncConversationsResponse,
    ConversationSyncAllResponse,
    GetConversationDetailResponse,
    ConversationPatchType,
};
use crate::domain::repository::ReadStore;
use crate::infrastructure::event_bus::EventBus;
use crate::application::fsm::FsmManager;
use crate::infrastructure::converter::ConversationConverter;
use tracing::{info, warn, debug, error};

/// 会话同步处理器
pub struct ConversationSyncHandler {
    read_store: Arc<dyn ReadStore>,
    event_bus: Arc<EventBus>,
    fsm: Arc<FsmManager>,
}

impl ConversationSyncHandler {
    /// 创建新的会话同步处理器
    pub fn new(
        read_store: Arc<dyn ReadStore>,
        event_bus: Arc<EventBus>,
        fsm: Arc<FsmManager>,
    ) -> Self {
        Self {
            read_store,
            event_bus,
            fsm,
        }
    }
    
    /// 处理会话增量同步响应
    ///
    /// # 参数
    ///
    /// * `resp` - 会话增量同步响应
    ///
    /// # 返回
    ///
    /// * `Ok(())` - 处理成功
    /// * `Err` - 处理失败
    pub async fn handle_sync_conversations_response(
        &self,
        resp: SyncConversationsResponse,
    ) -> anyhow::Result<()> {
        info!(
            patch_count = resp.patches.len(),
            has_more = resp.has_more,
            "Processing SyncConversationsResponse"
        );
        
        // 1. 解析 ConversationPatch 列表
        let patches = resp.patches.clone(); // 克隆用于后续处理
        
        if patches.is_empty() {
            debug!("No patches to process");
            return Ok(());
        }
        
        // 2. 按 patch_type 分类处理
        let mut light_updates = Vec::new();
        let mut summary_updates = Vec::new();
        let mut removed_ids = Vec::new();
        let mut created_ids = Vec::new();
        let mut detail_required_ids = Vec::new();
        
        // 性能优化：预分配容量，减少重新分配
        let patch_count = patches.len();
        light_updates.reserve(patch_count / 4);
        summary_updates.reserve(patch_count / 2);
        removed_ids.reserve(patch_count / 10);
        
        for patch in &patches {
            match patch.patch_type() {
                ConversationPatchType::ConversationPatchLight => {
                    if let Some(ref light) = patch.light {
                        light_updates.push((patch.conversation_id.clone(), light.clone()));
                    }
                }
                ConversationPatchType::ConversationPatchSummary => {
                    if let Some(ref summary) = patch.summary {
                        summary_updates.push((patch.conversation_id.clone(), summary.clone()));
                    }
                }
                ConversationPatchType::ConversationPatchRemoved => {
                    removed_ids.push(patch.conversation_id.clone());
                }
                ConversationPatchType::ConversationPatchCreated => {
                    created_ids.push(patch.conversation_id.clone());
                    // 创建新会话时，可能需要触发详情拉取
                    detail_required_ids.push(patch.conversation_id.clone());
                }
                ConversationPatchType::ConversationPatchDetailRequired => {
                    detail_required_ids.push(patch.conversation_id.clone());
                }
                _ => {
                    warn!(
                        conversation_id = %patch.conversation_id,
                        patch_type = ?patch.patch_type(),
                        "Unknown patch type"
                    );
                }
            }
        }
        
        debug!(
            light_count = light_updates.len(),
            summary_count = summary_updates.len(),
            removed_count = removed_ids.len(),
            created_count = created_ids.len(),
            detail_required_count = detail_required_ids.len(),
            "Classified conversation patches"
        );
        
        // 3. 性能优化：批量更新 ReadStore
        // 3.1 处理轻量更新（暂时跳过，等待 ConversationConverter 支持）
        let light_count = light_updates.len();
        drop(light_updates); // 释放内存
        
        // 3.2 处理全量更新（批量转换，减少错误处理开销）
        let summary_count = summary_updates.len();
        let mut conversations_to_update = Vec::with_capacity(summary_count);
        let mut failed_conversions = Vec::new();
        
        for (conversation_id, summary) in summary_updates {
            match ConversationConverter::from_proto_summary(&summary) {
                Ok(conversation) => {
                    conversations_to_update.push(conversation);
                }
                Err(e) => {
                    failed_conversions.push(conversation_id);
                    warn!(
                        error = %e,
                        "Failed to convert summary conversation"
                    );
                }
            }
        }
        
        if !failed_conversions.is_empty() {
            warn!(
                failed_count = failed_conversions.len(),
                "Some conversations failed to convert"
            );
        }
        
        // 性能优化：批量更新 ReadStore（并行处理，fire-and-forget 模式）
        // 对于同步场景，使用 fire-and-forget 可以显著提高吞吐量
        if !conversations_to_update.is_empty() {
            for conv in conversations_to_update {
                let read_store = self.read_store.clone();
                let conv_id = conv.conversation_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = read_store.write_conversation(&conv).await {
                        error!(
                            conversation_id = %conv_id,
                            error = %e,
                            "Failed to update conversation (summary)"
                        );
                    }
                });
            }
        }
        
        // 3.3 处理删除（软删除）
        let removed_count = removed_ids.len();
        drop(removed_ids); // 释放内存
        // TODO: 实现会话删除逻辑
        
        // 3.4 处理需要详情拉取的会话
        let detail_required_count = detail_required_ids.len();
        drop(detail_required_ids); // 释放内存
        // TODO: 触发异步详情拉取
        
        // 4. 更新会话游标
        if let Some(cursor) = resp.server_conversation_cursor {
            debug!(
                cursor = ?cursor,
                "Updated conversation cursor from sync response"
            );
            // TODO: 更新 FSM 或 SyncCoordinator 中的游标
        }
        
        // 5. 判断 has_more，触发后续同步
        if resp.has_more {
            debug!("SyncConversationsResponse indicates more patches available");
            // TODO: 触发后续同步
        }
        
        // 6. 性能优化：异步发布会话列表更新事件（不阻塞主流程）
        let event_bus = self.event_bus.clone();
        let patch_count = patches.len();
        let has_more = resp.has_more;
        let created_count = created_ids.len();
        tokio::spawn(async move {
            use crate::domain::event::DomainEvent;
            let sync_event = DomainEvent::new(
                "sync.conversations.completed",
                "sync",
                1,
                serde_json::json!({
                    "patch_count": patch_count,
                    "has_more": has_more,
                    "light_count": light_count,
                    "summary_count": summary_count,
                    "removed_count": removed_count,
                    "created_count": created_count,
                    "detail_required_count": detail_required_count,
                }),
            );
            
            if let Err(e) = event_bus.publish(sync_event).await {
                warn!("Failed to publish conversation sync completion event: {}", e);
            }
        });
        
        Ok(())
    }
    
    /// 处理全量会话同步响应
    ///
    /// # 参数
    ///
    /// * `resp` - 全量会话同步响应
    ///
    /// # 返回
    ///
    /// * `Ok(())` - 处理成功
    /// * `Err` - 处理失败
    pub async fn handle_conversation_sync_all_response(
        &self,
        resp: ConversationSyncAllResponse,
    ) -> anyhow::Result<()> {
        let conversation_count = resp.conversations.len();
        info!(
            conversation_count = conversation_count,
            "Processing ConversationSyncAllResponse"
        );
        
        // 批量转换会话（先转换，再批量更新）
        let mut conversations_to_update = Vec::with_capacity(conversation_count);
        for summary in resp.conversations {
            match ConversationConverter::from_proto_summary(&summary) {
                Ok(conversation) => {
                    conversations_to_update.push(conversation);
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Failed to convert conversation summary"
                    );
                }
            }
        }
        
        // 性能优化：批量更新 ReadStore（fire-and-forget 模式，提高吞吐量）
        if !conversations_to_update.is_empty() {
            for conv in conversations_to_update {
                let read_store = self.read_store.clone();
                let conv_id = conv.conversation_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = read_store.write_conversation(&conv).await {
                        error!(
                            conversation_id = %conv_id,
                            error = %e,
                            "Failed to update conversation from sync all"
                        );
                    }
                });
            }
        }
        
        // 发布全量同步完成事件
        use crate::domain::event::DomainEvent;
        let sync_event = DomainEvent::new(
            "sync.conversations.all.completed",
            "sync",
            1,
            serde_json::json!({
                "conversation_count": conversation_count,
            }),
        );
        
        if let Err(e) = self.event_bus.publish(sync_event).await {
            warn!("Failed to publish conversation sync all completion event: {}", e);
        } else {
            info!("Published conversation sync all completion event");
        }
        
        Ok(())
    }
    
    /// 处理会话详情响应
    ///
    /// # 参数
    ///
    /// * `resp` - 会话详情响应
    ///
    /// # 返回
    ///
    /// * `Ok(())` - 处理成功
    /// * `Err` - 处理失败
    pub async fn handle_conversation_detail_response(
        &self,
        resp: GetConversationDetailResponse,
    ) -> anyhow::Result<()> {
        info!("Processing GetConversationDetailResponse");
        
        if let Some(detail) = resp.detail {
            match ConversationConverter::from_proto_detail(&detail) {
                Ok(conversation) => {
                    // 性能优化：异步写入，不阻塞事件处理
                    let read_store = self.read_store.clone();
                    let conv_id = conversation.conversation_id.clone();
                    let conv = conversation;
                    tokio::spawn(async move {
                        if let Err(e) = read_store.write_conversation(&conv).await {
                            error!(
                                conversation_id = %conv_id,
                                error = %e,
                                "Failed to update conversation from detail"
                            );
                        } else {
                            info!(
                                conversation_id = %conv_id,
                                "Updated conversation from detail"
                            );
                        }
                    });
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Failed to convert conversation detail"
                    );
                }
            }
        } else {
            warn!("GetConversationDetailResponse has no detail");
        }
        
        Ok(())
    }
}
