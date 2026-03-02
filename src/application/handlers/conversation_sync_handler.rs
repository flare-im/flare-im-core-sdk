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
use crate::domain::repository::ConversationRepository;
use crate::infrastructure::event_bus::EventBus;
use crate::infrastructure::converter::ConversationConverter;
use tracing::{info, warn, debug, error};

/// 会话同步处理器
pub struct ConversationSyncHandler {
    conversation_repository: Arc<dyn ConversationRepository>,
    event_bus: Arc<EventBus>,
}

impl ConversationSyncHandler {
    /// 创建新的会话同步处理器
    pub fn new(
        conversation_repository: Arc<dyn ConversationRepository>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            conversation_repository,
            event_bus,
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
        
        // 确保数据持久化：顺序执行数据库写入，确保数据一致性
        if !conversations_to_update.is_empty() {
            for conv in conversations_to_update {
                // 尝试查找现有会话，如果存在则更新，否则保存
                let result = if self.conversation_repository.find_by_id(&conv.conversation_id).await.ok().flatten().is_some() {
                    self.conversation_repository.update(&conv).await
                } else {
                    self.conversation_repository.save(&conv).await
                };
                
                if let Err(e) = result {
                    error!(
                        conversation_id = %conv.conversation_id,
                        error = %e,
                        "Failed to update conversation (summary)"
                    );
                    // 考虑是否应该中断同步？目前策略是尽最大努力同步
                }
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
    
    /// 处理会话摘要列表（用于 Bootstrap 或 SyncAll）
    pub async fn handle_conversation_summaries(
        &self,
        summaries: Vec<flare_proto::common::ConversationSummary>,
    ) -> anyhow::Result<()> {
        let conversation_count = summaries.len();
        info!(
            conversation_count = conversation_count,
            "Processing conversation summaries"
        );
        
        // 批量转换会话
        let mut conversations_to_update = Vec::with_capacity(conversation_count);
        for summary in summaries {
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
        
        // 确保数据持久化
        if !conversations_to_update.is_empty() {
            for conv in conversations_to_update {
                let result = if self.conversation_repository.find_by_id(&conv.conversation_id).await.ok().flatten().is_some() {
                    self.conversation_repository.update(&conv).await
                } else {
                    self.conversation_repository.save(&conv).await
                };
                
                if let Err(e) = result {
                    error!(
                        conversation_id = %conv.conversation_id,
                        error = %e,
                        "Failed to update conversation"
                    );
                }
            }
        }
        
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
        
        // 确保数据持久化：顺序执行数据库写入，确保数据一致性
        if !conversations_to_update.is_empty() {
            for conv in conversations_to_update {
                // 尝试查找现有会话，如果存在则更新，否则保存
                let result = if self.conversation_repository.find_by_id(&conv.conversation_id).await.ok().flatten().is_some() {
                    self.conversation_repository.update(&conv).await
                } else {
                    self.conversation_repository.save(&conv).await
                };
                
                if let Err(e) = result {
                    error!(
                        conversation_id = %conv.conversation_id,
                        error = %e,
                        "Failed to update conversation from sync all"
                    );
                }
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
                    // 确保数据持久化：等待数据库写入完成
                    // 尝试查找现有会话，如果存在则更新，否则保存
                    let result = if self.conversation_repository.find_by_id(&conversation.conversation_id).await.ok().flatten().is_some() {
                        self.conversation_repository.update(&conversation).await
                    } else {
                        self.conversation_repository.save(&conversation).await
                    };
                    
                    if let Err(e) = result {
                        error!(
                            conversation_id = %conversation.conversation_id,
                            error = %e,
                            "Failed to update conversation from detail"
                        );
                    } else {
                        info!(
                            conversation_id = %conversation.conversation_id,
                            "Updated conversation from detail"
                        );
                    }
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

    /// 获取所有会话的游标（max_seq）
    pub async fn get_conversation_cursors(&self) -> anyhow::Result<std::collections::HashMap<String, i64>> {
        let mut cursor_map = std::collections::HashMap::new();
        let mut next_cursor = None;
        let limit = Some(100);

        loop {
            let result = self.conversation_repository.find_all(limit, next_cursor).await?;
            for conv in result.conversations {
                if conv.max_seq > 0 {
                    cursor_map.insert(conv.conversation_id, conv.max_seq as i64);
                }
            }
            
            if result.next_cursor.is_none() {
                break;
            }
            next_cursor = result.next_cursor;
        }
        
        Ok(cursor_map)
    }
}
