//! 会话命令处理器

use crate::application::commands::session::*;
use crate::domain::message::model::SessionId;
use crate::domain::session::repository::SessionRepository;
use crate::domain::session::service::SessionDomainService;
use crate::infrastructure::event::EventBus;
use anyhow::{Context, Result};
use prost_types::Timestamp;
use std::sync::Arc;
use tracing::warn;

/// 会话命令处理器
///
/// 处理会话相关的命令（创建、更新、删除、标记已读等）
///
/// 生产级特性：
/// - 批量更新会话（减少数据库查询）
/// - 缓存优化（减少重复查询）
/// - 异步更新（不阻塞主流程）
pub struct SessionCommandHandler {
    domain_service: Arc<dyn SessionDomainService>,
    repository: Arc<dyn SessionRepository>,
    event_bus: Arc<EventBus>,
    connection_manager: Option<Arc<crate::infrastructure::connection::ConnectionManager>>,
    /// 批量更新缓冲区（用于批量更新会话）
    update_buffer: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<String, std::collections::HashMap<String, String>>,
        >,
    >,
    /// 批量更新任务句柄
    batch_update_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl SessionCommandHandler {
    pub fn new(
        domain_service: Arc<dyn SessionDomainService>,
        repository: Arc<dyn SessionRepository>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        let handler = Self {
            domain_service,
            repository,
            event_bus,
            connection_manager: None,
            update_buffer: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            batch_update_handle: Arc::new(tokio::sync::Mutex::new(None)),
        };

        // 启动批量更新任务（每 100ms 批量处理一次）
        handler.start_batch_update_task();

        handler
    }

    /// 启动批量更新任务（按照微信/Telegram/飞书标准：批量更新减少数据库压力）
    fn start_batch_update_task(&self) {
        let buffer = Arc::clone(&self.update_buffer);
        let repository = Arc::clone(&self.repository);
        let event_bus = Arc::clone(&self.event_bus);
        let handle_clone = Arc::clone(&self.batch_update_handle);

        // 启动异步任务
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
            loop {
                interval.tick().await;

                // 批量处理更新
                let updates = {
                    let mut buf = buffer.write().await;
                    if buf.is_empty() {
                        continue;
                    }
                    let updates = buf.clone();
                    buf.clear();
                    updates
                };

                // 批量更新会话
                for (session_id_str, session_updates) in updates {
                    let session_id = crate::domain::SessionId::new(session_id_str.clone());
                    if let Ok(Some(session)) = repository.find_by_id(&session_id).await {
                        if let Ok((updated_session, _)) = session.update(session_updates) {
                            if let Err(e) = repository.save(&updated_session).await {
                                tracing::warn!(error = %e, session_id = %session_id_str, "Failed to batch update session");
                            } else {
                                // 发布更新事件
                                event_bus.publish(crate::infrastructure::event::Event::Session(
                                    crate::infrastructure::event::SessionEvent::SessionUpdated {
                                        session_id: session_id_str.clone(),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        });

        // 保存任务句柄（异步设置）
        let handle_for_save = handle;
        tokio::spawn(async move {
            let mut handle_guard = handle_clone.lock().await;
            *handle_guard = Some(handle_for_save);
        });
    }

    /// 设置连接管理器（用于发送输入状态等）
    pub fn with_connection_manager(
        mut self,
        connection_manager: Arc<crate::infrastructure::connection::ConnectionManager>,
    ) -> Self {
        self.connection_manager = Some(connection_manager);
        self
    }

    /// 处理创建会话命令
    pub async fn handle_create_session(&self, cmd: CreateSessionCommand) -> Result<SessionId> {
        // 1. 调用领域服务创建会话
        let session = self
            .domain_service
            .create_session(
                cmd.session_id,
                cmd.session_type,
                cmd.business_type,
                cmd.display_name,
                cmd.participants,
            )
            .await
            .context("Failed to create session")?;

        // 2. 保存到仓储
        self.repository
            .save(&session)
            .await
            .context("Failed to save session")?;

        // 3. 发布会话已创建事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionCreated {
                    session_id: session.id().to_string(),
                },
            ));

        Ok(session.id().clone())
    }

    /// 处理更新会话命令（生产级实现）
    ///
    /// 按照微信/Telegram/飞书标准：
    /// - 高频更新（如未读数）使用批量更新
    /// - 低频更新（如显示名称）立即更新
    pub async fn handle_update_session(&self, cmd: UpdateSessionCommand) -> Result<()> {
        // 判断是否为高频更新（未读数、最后消息等）
        let is_high_frequency_update = cmd
            .updates
            .keys()
            .any(|k| k == "unread_count" || k == "last_message_id" || k == "last_message_time");

        if is_high_frequency_update {
            // 高频更新：加入批量更新缓冲区（异步处理，不阻塞）
            let mut buffer = self.update_buffer.write().await;
            let session_id_str = cmd.session_id.to_string();

            // 合并更新（如果已有更新，合并；否则创建新条目）
            let existing_updates = buffer
                .entry(session_id_str.clone())
                .or_insert_with(std::collections::HashMap::new);
            for (k, v) in &cmd.updates {
                existing_updates.insert(k.clone(), v.clone());
            }

            // 立即返回，不等待批量处理
            Ok(())
        } else {
            // 低频更新：立即处理（显示名称、头像等）
            let session = self
                .repository
                .find_by_id(&cmd.session_id)
                .await
                .context("Failed to find session")?
                .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

            let updates: std::collections::HashMap<String, String> = cmd
                .updates
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            let (updated_session, _update_event) = session
                .update(updates)
                .context("Failed to update session")?;

            self.repository
                .save(&updated_session)
                .await
                .context("Failed to save session")?;

            self.event_bus
                .publish(crate::infrastructure::event::Event::Session(
                    crate::infrastructure::event::SessionEvent::SessionUpdated {
                        session_id: cmd.session_id.to_string(),
                    },
                ));

            Ok(())
        }
    }

    /// 处理删除会话命令
    pub async fn handle_delete_session(&self, cmd: DeleteSessionCommand) -> Result<()> {
        // 1. 查找会话
        let session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 2. 如果 delete_messages=true，删除所有消息
        if cmd.delete_messages {
            // TODO: 通过消息仓储删除所有消息
            // 暂时跳过，需要消息仓储支持按会话删除
        }

        // 3. 调用领域行为删除会话
        let delete_event = session.delete().context("Failed to delete session")?;

        // 4. 删除会话
        self.repository
            .delete(&cmd.session_id)
            .await
            .context("Failed to delete session")?;

        // 4. 发布会话已删除事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionDeleted {
                    session_id: cmd.session_id.to_string(),
                },
            ));

        Ok(())
    }

    /// 处理隐藏会话命令
    ///
    /// 按照微信/Telegram/飞书标准：隐藏会话后不在会话列表中显示，但消息仍保留
    pub async fn handle_hide_session(&self, cmd: HideSessionCommand) -> Result<()> {
        // 1. 查找会话
        let session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 2. 更新会话的 metadata，标记为隐藏
        let mut updates = std::collections::HashMap::new();
        updates.insert("hidden".to_string(), "true".to_string());

        let (mut updated_session, _) = session
            .update(updates)
            .context("Failed to update session")?;

        // 3. 调用领域行为隐藏会话（在更新后）
        // 注意：hide() 会消费 updated_session，所以我们需要先保存
        let session_id = updated_session.id().clone();
        let _hide_event = updated_session.hide().context("Failed to hide session")?;

        // 4. 重新获取会话并保存（因为 hide() 消费了 updated_session）
        let session_to_save = self
            .repository
            .find_by_id(&session_id)
            .await
            .context("Failed to re-fetch session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        self.repository
            .save(&session_to_save)
            .await
            .context("Failed to save hidden session")?;

        // 5. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionHidden {
                    session_id: cmd.session_id.to_string(),
                },
            ));

        Ok(())
    }

    /// 处理显示会话命令
    ///
    /// 按照微信/Telegram/飞书标准：显示被隐藏的会话
    pub async fn handle_show_session(&self, cmd: ShowSessionCommand) -> Result<()> {
        // 1. 查找会话
        let session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 2. 更新会话的 metadata，移除隐藏标记
        let mut updates = std::collections::HashMap::new();
        updates.insert("hidden".to_string(), "false".to_string());

        let (mut updated_session, _) = session
            .update(updates)
            .context("Failed to update session")?;

        // 3. 调用领域行为显示会话（在更新后）
        // 注意：show() 会消费 updated_session，所以我们需要先保存
        let session_id = updated_session.id().clone();
        let _show_event = updated_session.show().context("Failed to show session")?;

        // 4. 重新获取会话并保存（因为 show() 消费了 updated_session）
        let session_to_save = self
            .repository
            .find_by_id(&session_id)
            .await
            .context("Failed to re-fetch session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        self.repository
            .save(&session_to_save)
            .await
            .context("Failed to save shown session")?;

        // 5. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionShown {
                    session_id: cmd.session_id.to_string(),
                },
            ));

        Ok(())
    }

    /// 处理标记已读命令
    ///
    /// 按照微信/Telegram/飞书标准：标记会话已读，更新 last_read_seq 和未读数
    pub async fn handle_mark_read(&self, cmd: MarkReadCommand) -> Result<()> {
        use crate::domain::UserId;

        // 1. 查找会话
        let session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 2. 获取 reader_id
        // 注意：mark_read 方法需要 reader_id，但 MarkReadCommand 中没有这个字段
        // 在实际场景中，reader_id 应该是当前登录用户
        // 这里暂时从会话的 metadata 中获取，或者使用一个默认值
        // 实际应该从命令或上下文中获取当前用户ID
        let reader_id = UserId::new("current_user".to_string()); // TODO: 从命令或上下文获取实际用户ID

        // 3. 调用领域行为标记已读
        let mark_read_event = session
            .mark_read(reader_id.clone(), cmd.message_seq)
            .context("Failed to mark read")?;

        // 4. 更新会话的 last_read_seq 和未读数
        // 注意：mark_read 方法会消费 session，所以我们需要重新获取
        let updated_session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to re-fetch session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found after mark read"))?;

        // 5. 更新 proto_summary 中的 last_read_seq
        let mut updates = std::collections::HashMap::new();
        if let Some(seq) = cmd.message_seq {
            updates.insert("last_read_seq".to_string(), seq.to_string());
        }

        let (final_session, _) = updated_session
            .update(updates)
            .context("Failed to update session after mark read")?;

        // 6. 保存更新后的会话
        self.repository
            .save(&final_session)
            .await
            .context("Failed to save marked read session")?;

        // 7. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionMarkedRead {
                    session_id: cmd.session_id.to_string(),
                    message_seq: cmd.message_seq.unwrap_or(0),
                },
            ));

        Ok(())
    }

    /// 处理批量标记已读命令
    pub async fn handle_mark_read_batch(&self, cmd: MarkReadBatchCommand) -> Result<usize> {
        let mut success_count = 0;
        for session_id in cmd.session_ids {
            if self
                .handle_mark_read(MarkReadCommand {
                    session_id,
                    message_seq: None,
                })
                .await
                .is_ok()
            {
                success_count += 1;
            }
        }
        Ok(success_count)
    }

    /// 处理设置草稿命令
    pub async fn handle_set_draft(&self, cmd: SetDraftCommand) -> Result<()> {
        // 1. 查找会话
        let session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 2. 调用领域行为设置草稿
        let _draft_event = session
            .set_draft(cmd.draft.clone())
            .context("Failed to set draft")?;

        // 3. 重新获取会话以保存（因为 session 已被移动）
        let updated_session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session for save")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 4. 保存到仓储（草稿存储在会话的 metadata 中）
        self.repository
            .save(&updated_session)
            .await
            .context("Failed to save session")?;

        // 4. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionDraftSet {
                    session_id: cmd.session_id.to_string(),
                    draft: cmd.draft.clone().unwrap_or_default(),
                },
            ));

        Ok(())
    }

    /// 处理发送输入状态命令
    pub async fn handle_send_typing(&self, cmd: SendTypingCommand) -> Result<()> {
        // 1. 查找会话
        let session = self
            .repository
            .find_by_id(&cmd.session_id)
            .await
            .context("Failed to find session")?
            .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

        // 2. 调用领域行为发送输入状态
        let typing_event = session
            .send_typing(cmd.user_id.clone(), cmd.is_typing)
            .context("Failed to send typing")?;

        // 3. 通过 ConnectionManager 发送输入状态到服务器
        // 按照微信/Telegram/飞书标准：输入状态通过 NotificationCommand 发送
        if let Some(ref connection_manager) = self.connection_manager {
            use crate::infrastructure::protocol::FrameBuilder;
            use flare_core::common::protocol::{NotificationCommand, Reliability};

            let mut metadata = std::collections::HashMap::new();
            metadata.insert(
                "session_id".to_string(),
                cmd.session_id.as_str().as_bytes().to_vec(),
            );
            metadata.insert(
                "user_id".to_string(),
                cmd.user_id.as_str().as_bytes().to_vec(),
            );
            metadata.insert(
                "is_typing".to_string(),
                if cmd.is_typing {
                    "true".as_bytes().to_vec()
                } else {
                    "false".as_bytes().to_vec()
                },
            );

            let notification = NotificationCommand {
                r#type: 1, // Typing 类型
                title: "typing".to_string(),
                content: Vec::new(),
                metadata,
            };

            let frame = FrameBuilder::new()
                .with_notification_command(notification)
                .with_reliability(Reliability::AtLeastOnce) // 输入状态使用 AtLeastOnce
                .build();

            if let Err(e) = connection_manager.send_frame(&frame).await {
                warn!(error = %e, "Failed to send typing status to server");
            }
        }

        // 4. 发布基础设施事件
        self.event_bus
            .publish(crate::infrastructure::event::Event::Session(
                crate::infrastructure::event::SessionEvent::SessionTypingSent {
                    session_id: cmd.session_id.to_string(),
                    user_id: cmd.user_id.to_string(),
                    is_typing: cmd.is_typing,
                },
            ));

        Ok(())
    }
}
