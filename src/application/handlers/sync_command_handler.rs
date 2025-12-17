//! 同步命令处理器
//!
//! 按照微信、Telegram、飞书的标准实现消息和会话同步

use crate::application::commands::sync::*;
use crate::domain::message::repository::MessageRepository;
use crate::domain::session::repository::SessionRepository;
use crate::domain::sync::model::{Sync, SyncCursor, SyncStatus, SyncType};
use crate::domain::sync::repository::SyncRepository;
use crate::domain::sync::service::SyncDomainService;
use crate::infrastructure::connection::ConnectionManager;
use crate::infrastructure::event::EventBus;
use crate::infrastructure::protocol::RequestManager;
use crate::infrastructure::protocol::frame_builder::FrameBuilder;
use anyhow::{Context, Result};
use flare_core::common::protocol::{CustomCommand, Reliability};
use flare_proto::{Message as ProtoMessage, common::SessionSummary as ProtoSessionSummary};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// 同步命令处理器
///
/// 处理同步相关的命令（同步消息、同步会话等）
///
/// 实现标准：
/// - 微信：增量同步 + 全量同步，基于 seq 的增量拉取
/// - Telegram：分页同步，使用 cursor 进行分页
/// - 飞书：智能同步，根据离线时长决定全量/增量
pub struct SyncCommandHandler {
    domain_service: Arc<dyn SyncDomainService>,
    repository: Arc<dyn SyncRepository>,
    message_repository: Arc<dyn MessageRepository>,
    session_repository: Arc<dyn SessionRepository>,
    event_bus: Arc<EventBus>,
    request_manager: Arc<RequestManager>,
    connection_manager: Option<Arc<ConnectionManager>>,
}

impl SyncCommandHandler {
    pub fn new(
        domain_service: Arc<dyn SyncDomainService>,
        repository: Arc<dyn SyncRepository>,
        message_repository: Arc<dyn MessageRepository>,
        session_repository: Arc<dyn SessionRepository>,
        event_bus: Arc<EventBus>,
        request_manager: Arc<RequestManager>,
    ) -> Self {
        Self {
            domain_service,
            repository,
            message_repository,
            session_repository,
            event_bus,
            request_manager,
            connection_manager: None,
        }
    }

    /// 设置连接管理器（用于发送同步请求）
    pub fn with_connection_manager(mut self, connection_manager: Arc<ConnectionManager>) -> Self {
        self.connection_manager = Some(connection_manager);
        self
    }

    /// 处理同步消息命令
    ///
    /// 按照微信/Telegram/飞书标准：
    /// 1. 增量同步：基于 last_seq 拉取新消息
    /// 2. 全量同步：拉取最近 N 条消息（默认 50 条）
    /// 3. 分页同步：使用 cursor 进行分页
    pub async fn handle_sync_messages(
        &self,
        cmd: SyncMessagesCommand,
    ) -> Result<crate::domain::sync::SyncResult> {
        info!(
            session_id = ?cmd.session_id,
            sync_type = ?cmd.sync_type,
            after_seq = ?cmd.after_seq,
            "Starting message sync"
        );

        // 1. 创建同步聚合根
        let sync = self
            .domain_service
            .create_sync(cmd.session_id.clone(), cmd.sync_type)
            .await
            .context("Failed to create sync")?;

        // 2. 开始同步（更新状态为 InProgress）
        let _sync_started_event = sync.start()?;
        // 注意：sync.start() 消费了 sync，所以我们需要重新创建或克隆
        // 但 start() 方法返回的是事件，不是 sync，所以我们需要重新获取
        // 这里简化处理，直接使用 update_status 创建一个新的 sync
        let mut sync = Sync::new(cmd.session_id.clone(), cmd.sync_type);
        sync = sync.update_status(SyncStatus::InProgress);

        // 3. 发布同步开始事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Sync(
                crate::infrastructure::event::SyncEvent::SyncStarted {
                    sync_type: format!("{:?}", sync.sync_type()),
                    estimated_sessions: None,
                },
            ));

        // 4. 获取当前游标（用于增量同步）
        let mut cursor = if let Some(session_id) = &cmd.session_id {
            // 尝试从仓储获取已有游标
            if let Ok(Some(existing_sync)) = self.repository.find_by_session(session_id).await {
                existing_sync.cursor().cloned()
            } else {
                None
            }
        } else {
            None
        };

        // 5. 构建同步请求 Frame
        let sync_result = match cmd.sync_type {
            SyncType::Full => {
                // 全量同步：拉取最近 50 条消息（微信标准）
                self.sync_messages_full(&cmd, &mut cursor).await
            }
            SyncType::Incremental => {
                // 增量同步：基于 last_seq 拉取新消息
                self.sync_messages_incremental(&cmd, &mut cursor).await
            }
            SyncType::Session => {
                // 会话同步不应该调用消息同步
                return Err(anyhow::anyhow!("Invalid sync type for message sync"));
            }
        };

        match sync_result {
            Ok((message_count, has_more, mut updated_cursor)) => {
                // 6. 更新同步游标
                if let Some(ref mut cursor) = updated_cursor {
                    let mut completed_sync = Sync::new(cmd.session_id.clone(), cmd.sync_type);
                    completed_sync = completed_sync.update_status(SyncStatus::Completed);
                    let sync_for_result = completed_sync.clone();
                    let completed_sync_for_save = completed_sync.clone();
                    let _sync_completed_event = completed_sync.complete(Some(cursor.clone()))?;

                    // 7. 保存同步状态
                    if let Err(e) = self.repository.save(&completed_sync_for_save).await {
                        warn!(error = %e, "Failed to save sync state");
                    }

                    // 8. 发布同步完成事件
                    self.event_bus
                        .publish(crate::infrastructure::event::Event::Sync(
                            crate::infrastructure::event::SyncEvent::SyncCompleted {
                                sessions: 0,
                                messages: message_count,
                                duration_ms: 0, // TODO: 计算实际耗时
                                has_background_sync: has_more,
                            },
                        ));

                    info!(
                        session_id = ?cmd.session_id,
                        message_count,
                        has_more,
                        "Message sync completed"
                    );

                    // 9. 返回同步结果
                    Ok(sync_for_result.to_result(message_count, has_more))
                } else {
                    Err(anyhow::anyhow!("Failed to update cursor"))
                }
            }
            Err(e) => {
                error!(error = %e, "Message sync failed");

                // 更新同步状态为失败
                let mut failed_sync = Sync::new(cmd.session_id.clone(), cmd.sync_type);
                failed_sync = failed_sync.update_status(SyncStatus::Failed);
                let failed_sync_clone = failed_sync.clone();
                let _sync_failed_event = failed_sync.fail(e.to_string())?;
                if let Err(save_err) = self.repository.save(&failed_sync_clone).await {
                    warn!(error = %save_err, "Failed to save failed sync state");
                }

                // 发布同步失败事件
                self.event_bus
                    .publish(crate::infrastructure::event::Event::Sync(
                        crate::infrastructure::event::SyncEvent::SyncFailed {
                            error: e.to_string(),
                            sessions_synced: None,
                            messages_synced: None,
                        },
                    ));

                Err(e)
            }
        }
    }

    /// 全量同步消息（拉取最近 N 条）
    async fn sync_messages_full(
        &self,
        cmd: &SyncMessagesCommand,
        cursor: &mut Option<SyncCursor>,
    ) -> Result<(usize, bool, Option<SyncCursor>)> {
        let connection_manager = self
            .connection_manager
            .as_ref()
            .context("ConnectionManager not set")?;

        // 构建同步请求（CustomCommand: SyncMessages）
        let (request_id, mut receiver) = self.request_manager.create_request().await;

        let sync_request = CustomCommand {
            name: "SyncMessages".to_string(),
            data: Vec::new(), // 占位符：需要根据实际的 protobuf 定义来构建
            metadata: {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
                if let Some(session_id) = &cmd.session_id {
                    metadata.insert(
                        "session_id".to_string(),
                        session_id.as_str().as_bytes().to_vec(),
                    );
                }
                metadata.insert("sync_type".to_string(), "full".as_bytes().to_vec());
                metadata.insert("limit".to_string(), "50".as_bytes().to_vec()); // 微信标准：最近 50 条
                metadata
            },
        };

        let frame = FrameBuilder::new()
            .with_custom_command(sync_request)
            .with_metadata_str("request_id".to_string(), request_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        // 发送请求
        connection_manager
            .send_frame(&frame)
            .await
            .context("Failed to send sync request")?;

        // 等待响应（超时 30 秒）
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), receiver)
            .await
            .context("Sync request timeout")?
            .context("Failed to receive sync response")?;

        // 解析响应
        self.parse_sync_messages_response(response, cmd, cursor)
            .await
    }

    /// 增量同步消息（基于 seq）
    async fn sync_messages_incremental(
        &self,
        cmd: &SyncMessagesCommand,
        cursor: &mut Option<SyncCursor>,
    ) -> Result<(usize, bool, Option<SyncCursor>)> {
        let connection_manager = self
            .connection_manager
            .as_ref()
            .context("ConnectionManager not set")?;

        // 确定起始 seq
        let start_seq = if let Some(after_seq) = cmd.after_seq {
            after_seq + 1
        } else if let Some(cursor) = cursor.as_ref() {
            cursor.last_seq.map(|seq| seq + 1).unwrap_or(1)
        } else {
            1
        };

        let (request_id, mut receiver) = self.request_manager.create_request().await;

        let sync_request = CustomCommand {
            name: "SyncMessages".to_string(),
            data: Vec::new(),
            metadata: {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
                if let Some(session_id) = &cmd.session_id {
                    metadata.insert(
                        "session_id".to_string(),
                        session_id.as_str().as_bytes().to_vec(),
                    );
                }
                metadata.insert("sync_type".to_string(), "incremental".as_bytes().to_vec());
                metadata.insert(
                    "after_seq".to_string(),
                    start_seq.to_string().as_bytes().to_vec(),
                );
                metadata.insert("limit".to_string(), "100".as_bytes().to_vec()); // Telegram 标准：每次最多 100 条
                metadata
            },
        };

        let frame = FrameBuilder::new()
            .with_custom_command(sync_request)
            .with_metadata_str("request_id".to_string(), request_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        connection_manager
            .send_frame(&frame)
            .await
            .context("Failed to send incremental sync request")?;

        let response = tokio::time::timeout(std::time::Duration::from_secs(30), receiver)
            .await
            .context("Incremental sync request timeout")?
            .context("Failed to receive incremental sync response")?;

        self.parse_sync_messages_response(response, cmd, cursor)
            .await
    }

    /// 解析同步消息响应
    async fn parse_sync_messages_response(
        &self,
        response: flare_core::common::protocol::Frame,
        cmd: &SyncMessagesCommand,
        cursor: &mut Option<SyncCursor>,
    ) -> Result<(usize, bool, Option<SyncCursor>)> {
        // 从响应中提取消息列表
        // 注意：这里需要根据实际的响应格式来解析
        // 假设响应 Frame 的 payload 中包含消息列表

        let messages = if let Some(flare_core::common::protocol::Command {
            r#type:
                Some(flare_core::common::protocol::flare::core::commands::command::Type::Custom(
                    custom_cmd,
                )),
        }) = &response.command
        {
            // 解析 CustomCommand 的 payload
            // 假设 payload 是 protobuf 编码的 SyncMessagesResponse
            // 这里需要根据实际的 protobuf 定义来解析
            Vec::<ProtoMessage>::new() // 占位符
        } else {
            return Err(anyhow::anyhow!("Invalid response format"));
        };

        // 保存消息到本地存储
        let mut saved_count = 0;
        for proto_msg in &messages {
            if let Ok(domain_msg) = crate::domain::message::Message::from_proto(proto_msg.clone()) {
                if let Err(e) = self.message_repository.save(&domain_msg).await {
                    warn!(error = %e, message_id = %domain_msg.id(), "Failed to save message during sync");
                } else {
                    saved_count += 1;
                }
            }
        }

        // 更新游标
        let updated_cursor = if let Some(session_id) = &cmd.session_id {
            let mut new_cursor = cursor
                .clone()
                .unwrap_or_else(|| SyncCursor::new(session_id.to_string()));

            // 更新 last_seq 和 max_seq
            if let Some(last_msg) = messages.last() {
                if last_msg.seq > 0 {
                    new_cursor.update(Some(last_msg.seq as i64), None, None);
                }
            }

            Some(new_cursor)
        } else {
            cursor.clone()
        };

        let has_more = messages.len() >= 100; // 如果返回了 100 条，可能还有更多

        Ok((saved_count, has_more, updated_cursor))
    }

    /// 处理同步会话命令
    ///
    /// 按照微信/Telegram/飞书标准：
    /// 1. 分页同步：使用 cursor 进行分页（Telegram 标准）
    /// 2. 增量同步：只同步变更的会话（飞书标准）
    /// 3. 全量同步：首次登录时同步所有会话（微信标准）
    pub async fn handle_sync_sessions(
        &self,
        cmd: SyncSessionsCommand,
    ) -> Result<crate::application::vo::session::SessionSyncResultVO> {
        info!(cursor = ?cmd.cursor, "Starting session sync");

        let connection_manager = self
            .connection_manager
            .as_ref()
            .context("ConnectionManager not set")?;

        // 构建同步请求
        let (request_id, mut receiver) = self.request_manager.create_request().await;

        let sync_request = CustomCommand {
            name: "ListSessions".to_string(),
            data: Vec::new(),
            metadata: {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
                if let Some(ref cursor) = cmd.cursor {
                    metadata.insert("cursor".to_string(), cursor.as_bytes().to_vec());
                }
                metadata.insert("limit".to_string(), "50".to_string().as_bytes().to_vec()); // 每次 50 个会话
                metadata
            },
        };

        let frame = FrameBuilder::new()
            .with_custom_command(sync_request)
            .with_metadata_str("request_id".to_string(), request_id.clone())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        // 发送请求
        connection_manager
            .send_frame(&frame)
            .await
            .context("Failed to send session sync request")?;

        // 等待响应
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), receiver)
            .await
            .context("Session sync request timeout")?
            .context("Failed to receive session sync response")?;

        // 解析响应
        let sessions = if let Some(flare_core::common::protocol::Command {
            r#type:
                Some(flare_core::common::protocol::flare::core::commands::command::Type::Custom(
                    custom_cmd,
                )),
        }) = &response.command
        {
            // 解析会话列表
            Vec::<ProtoSessionSummary>::new() // 占位符，需要根据实际响应格式解析
        } else {
            return Err(anyhow::anyhow!("Invalid response format"));
        };

        // 保存会话到本地存储
        let mut saved_count = 0;
        let mut session_vos = Vec::new();

        for proto_session in &sessions {
            if let Ok(domain_session) =
                crate::domain::session::Session::from_proto(proto_session.clone())
            {
                if let Err(e) = self.session_repository.save(&domain_session).await {
                    warn!(error = %e, session_id = %domain_session.id(), "Failed to save session during sync");
                } else {
                    saved_count += 1;
                    // 转换为 VO
                    let summary = domain_session.to_summary();
                    session_vos.push(crate::application::vo::session::SessionVO::from(summary));
                }
            }
        }

        // 从响应中提取 next_cursor
        let next_cursor = response
            .metadata
            .get("next_cursor")
            .and_then(|v| String::from_utf8(v.clone()).ok());

        let has_more = sessions.len() >= 50; // 如果返回了 50 个，可能还有更多

        // 发布同步完成事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Sync(
                crate::infrastructure::event::SyncEvent::SyncCompleted {
                    sessions: saved_count,
                    messages: 0,
                    duration_ms: 0, // TODO: 计算实际耗时
                    has_background_sync: has_more,
                },
            ));

        info!(count = saved_count, has_more, "Session sync completed");

        Ok(crate::application::vo::session::SessionSyncResultVO {
            sessions: session_vos,
            has_more,
            next_cursor,
            count: saved_count,
        })
    }

    /// 处理同步响应（用于 message_listener）
    pub async fn handle_response(&self, frame: flare_core::common::protocol::Frame) -> Result<()> {
        // 从 frame.metadata 中提取 request_id
        let request_id = frame
            .metadata
            .get("request_id")
            .and_then(|v| String::from_utf8(v.clone()).ok())
            .context("Response frame missing request_id")?;

        // 完成请求
        self.request_manager
            .complete_request(&request_id, frame)
            .await
            .context("Failed to complete sync request")?;

        Ok(())
    }
}
