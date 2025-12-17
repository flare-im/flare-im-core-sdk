//! 消息查询处理器

use crate::application::queries::message::*;
use crate::domain::message::Message as DomainMessage;
use crate::domain::message::repository::MessageRepository;
use anyhow::{Context, Result};
use flare_proto::Message as ProtoMessage;

/// 消息查询处理器
///
/// 处理消息相关的查询（获取消息列表、搜索消息等）
pub struct MessageQueryHandler {
    repository: Arc<dyn MessageRepository>,
}

impl MessageQueryHandler {
    pub fn new(repository: Arc<dyn MessageRepository>) -> Self {
        Self { repository }
    }

    /// 处理获取消息列表查询
    pub async fn handle_get_messages(&self, query: GetMessagesQuery) -> Result<Vec<ProtoMessage>> {
        let domain_messages = self
            .repository
            .find_by_session(
                &query.session_id,
                query.limit,
                query.before_message_id.as_ref(),
            )
            .await
            .context("Failed to get messages")?;

        Ok(domain_messages
            .into_iter()
            .map(|msg| msg.to_proto())
            .collect())
    }

    /// 处理获取消息查询
    pub async fn handle_get_message(&self, query: GetMessageQuery) -> Result<Option<ProtoMessage>> {
        let domain_message = self
            .repository
            .find_by_id(&query.message_id)
            .await
            .context("Failed to get message")?;

        Ok(domain_message.map(|msg| msg.to_proto()))
    }

    /// 处理批量获取消息查询
    pub async fn handle_get_messages_batch(
        &self,
        query: GetMessagesBatchQuery,
    ) -> Result<Vec<ProtoMessage>> {
        let mut results = Vec::new();
        for msg_id in query.message_ids {
            if let Ok(Some(msg)) = self.repository.find_by_id(&msg_id).await {
                results.push(msg.to_proto());
            }
        }
        Ok(results)
    }

    /// 处理搜索消息查询
    ///
    /// 按照微信/Telegram/飞书标准：支持关键词搜索，可指定会话范围
    pub async fn handle_search_messages(
        &self,
        query: SearchMessagesQuery,
    ) -> Result<Vec<ProtoMessage>> {
        use crate::domain::SessionId;

        // 如果指定了会话 ID，只在该会话中搜索
        if let Some(session_id_str) = &query.session_id {
            let session_id = SessionId::new(session_id_str.clone());
            let messages = self
                .repository
                .find_by_session(&session_id, query.limit.unwrap_or(50), None)
                .await
                .context("Failed to get messages for search")?;

            // 在内存中过滤关键词（实际应该由存储层实现全文搜索）
            let keyword_lower = query.keyword.to_lowercase();
            let filtered: Vec<_> = messages
                .into_iter()
                .filter(|msg| {
                    // 搜索消息内容
                    let proto = msg.to_proto();
                    if let Some(content) = &proto.content {
                        match &content.content {
                            Some(
                                flare_proto::flare::common::v1::message_content::Content::Text(
                                    text,
                                ),
                            ) => text.text.to_lowercase().contains(&keyword_lower),
                            _ => false,
                        }
                    } else {
                        false
                    }
                })
                .take(query.limit.unwrap_or(50))
                .collect();

            Ok(filtered.into_iter().map(|msg| msg.to_proto()).collect())
        } else {
            // 全局搜索：遍历所有会话（性能较差，实际应该由存储层实现）
            // 这里简化实现，只返回空结果
            // 实际应该调用存储层的全局搜索方法
            Ok(Vec::new())
        }
    }

    /// 处理获取历史消息查询
    ///
    /// 按照微信/Telegram/飞书标准：支持基于 seq 或 message_id 的历史消息查询
    pub async fn handle_get_history(&self, query: GetHistoryQuery) -> Result<Vec<ProtoMessage>> {
        // 优先使用 before_seq，如果没有则使用 before_message_id
        let messages = if let Some(before_seq) = query.before_seq {
            // 基于 seq 查询：获取指定 seq 之前的消息
            // 注意：当前仓储接口不支持基于 seq 查询，需要扩展
            // 这里暂时使用 find_by_session，然后过滤
            let all_messages = self
                .repository
                .find_by_session(&query.session_id, query.limit * 2, None)
                .await
                .context("Failed to get messages for history")?;

            all_messages
                .into_iter()
                .filter(|msg| {
                    let proto = msg.to_proto();
                    proto.seq > 0 && (proto.seq as i64) < before_seq
                })
                .take(query.limit)
                .collect()
        } else if let Some(ref before_msg_id) = query.before_message_id {
            // 基于 message_id 查询：获取指定消息之前的消息
            self.repository
                .find_by_session(&query.session_id, query.limit, Some(before_msg_id))
                .await
                .context("Failed to get history messages")?
        } else {
            // 没有指定条件，返回最近的消息
            self.repository
                .find_by_session(&query.session_id, query.limit, None)
                .await
                .context("Failed to get recent messages")?
        };

        // 如果指定了 after_seq，进一步过滤
        let filtered = if let Some(after_seq) = query.after_seq {
            messages
                .into_iter()
                .filter(|msg| {
                    let proto = msg.to_proto();
                    if proto.seq > 0 {
                        (proto.seq as i64) > after_seq
                    } else {
                        false
                    }
                })
                .collect()
        } else {
            messages
        };

        Ok(filtered.into_iter().map(|msg| msg.to_proto()).collect())
    }
}

use std::sync::Arc;
