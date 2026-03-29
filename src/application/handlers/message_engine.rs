//! 消息业务处理器（Message Handler）
//!
//! EventBus 下的一支：负责消息发送/撤回/编辑/删除、已读/输入/反应/置顶/标记及查询。
//! 依赖 Command、Repository、ReliableQueue，通过 EventBus 与 Dispatcher 协同。

use std::sync::Arc;
use std::time::Duration;

use crate::application::commands::{EditMessageCommand, RecallMessageCommand, SendMessageCommand};
use crate::application::handlers::MessageQueryHandler;
use crate::application::queries::{GetMessagesQuery, SearchMessagesQuery};
use crate::core::CurrentUserIdStore;
use crate::domain::UserReader;
use crate::error::{ErrorCode, FlareError, Result};
use crate::event::EventBus;
use crate::middleware::MiddlewareChain;
use crate::model::content_builder::{BuiltContent, ContentBuilder};
use crate::model::event::{
    Event, EventType, MarkEvent, MessageDeleteEvent, PinEvent, ReactionEvent, ReadReceiptEvent,
    TypingEvent, UnmarkEvent, UnpinEvent,
};
use crate::model::message::{IMMessage, MarkType, ReactionAction, SendAck};
use crate::protocol::PacketSender;
use crate::reliable_queue::ReliableSendQueue;
use crate::store::MessageStore;
use crate::util::REQUEST_TIMEOUT_SECS;
use flare_proto::common::event::Payload as EventPayload;

fn timeout() -> Duration {
    Duration::from_secs(REQUEST_TIMEOUT_SECS)
}

pub struct MessageEngine {
    pub(super) sender: Arc<PacketSender>,
    pub(super) store: Arc<dyn MessageStore>,
    pub(super) query_handler: Arc<MessageQueryHandler>,
    pub(super) chain: Arc<MiddlewareChain>,
    pub(super) current_user_id: CurrentUserIdStore,
    pub(super) profile_reader: Arc<dyn UserReader>,
    pub(super) reliable_queue: Option<Arc<ReliableSendQueue>>,
    #[allow(dead_code)]
    pub(super) bus: Option<EventBus>,
}

impl MessageEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sender: Arc<PacketSender>,
        store: Arc<dyn MessageStore>,
        query_handler: Arc<MessageQueryHandler>,
        chain: Arc<MiddlewareChain>,
        current_user_id: CurrentUserIdStore,
        profile_reader: Arc<dyn UserReader>,
        reliable_queue: Option<Arc<ReliableSendQueue>>,
        bus: Option<EventBus>,
    ) -> Self {
        Self {
            sender,
            store,
            query_handler,
            chain,
            current_user_id,
            profile_reader,
            reliable_queue,
            bus,
        }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(uid)
    }

    /// 统一发送入口：接收 IMMessage，走可靠队列或直发。
    /// - 有可靠队列时：仅入队后立即返回「已入队」回执，真实 SendAck / SendFailed 通过 EventBus 的
    ///   MessageEvent::SendAck / MessageEvent::SendFailed 回调处理。
    /// - 无可靠队列时：直接发送并等待回执后返回。
    pub async fn send_message(&self, message: IMMessage) -> Result<SendAck> {
        if let Some(queue) = &self.reliable_queue {
            SendMessageCommand::new(message.clone())
                .execute_via_queue(queue.as_ref())
                .await?;
            Ok(SendAck {
                client_msg_id: message.client_msg_id,
                server_msg_id: String::new(),
                seq: 0,
                conversation_id: message.conversation_id,
                success: true,
                ..Default::default()
            })
        } else {
            SendMessageCommand::new(message)
                .execute(&self.sender, self.store.as_ref(), &self.chain)
                .await
        }
    }

    /// 将上层传入的 message_id（统一为 client_msg_id）解析为 (conversation_id, server_msg_id)。
    /// 内部与服务端交互使用 server_msg_id；查库时先按 client_msg_id 查，再按 server_id 查。
    async fn resolve_message_id(&self, message_id: &str) -> Result<(String, String)> {
        let msg = match self.store.get_by_client_msg_id(message_id).await? {
            Some(m) => m,
            None => self.store.get(message_id).await?.ok_or_else(|| {
                FlareError::localized(
                    ErrorCode::MessageNotFound,
                    format!("message not found: {}", message_id),
                )
            })?,
        };
        let server_id = if msg.server_id.is_empty() {
            msg.client_msg_id.clone()
        } else {
            msg.server_id.clone()
        };
        Ok((msg.conversation_id().to_string(), server_id))
    }

    pub async fn recall(&self, message_id: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        RecallMessageCommand {
            conversation_id: conv_id,
            server_msg_id: server_id,
        }
        .execute(&self.sender, self.store.as_ref())
        .await
    }

    /// 编辑消息；message_id 为上层传入的 client_msg_id，内部解析为 server_msg_id 再发服务端。
    pub async fn edit(
        &self,
        conversation_id: &str,
        message_id: &str,
        new_content: Vec<u8>,
    ) -> Result<()> {
        let (_conv_id, server_msg_id) = self.resolve_message_id(message_id).await?;
        EditMessageCommand {
            conversation_id: conversation_id.to_string(),
            server_msg_id,
            new_content,
        }
        .execute(&self.sender)
        .await
    }

    pub async fn edit_content(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        content: BuiltContent,
    ) -> Result<()> {
        self.edit(conversation_id, server_msg_id, content.encode())
            .await
    }

    pub async fn edit_text(&self, message_id: &str, text: &str) -> Result<()> {
        let (conv_id, _) = self.resolve_message_id(message_id).await?;
        self.edit_content(&conv_id, message_id, ContentBuilder::text(text).build())
            .await
    }

    pub async fn delete(&self, message_id: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        let event = Event {
            conversation_id: conv_id.clone(),
            r#type: EventType::EventMessageDelete as i32,
            payload: Some(EventPayload::Delete(MessageDeleteEvent {
                server_msg_id: server_id.clone(),
                delete_type: None,
                scope: None,
                reason: None,
                ..Default::default()
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        self.store.delete(&server_id).await?;
        Ok(())
    }

    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.mark_read_with_ids(conversation_id, Vec::new(), read_seq)
            .await
    }

    pub async fn mark_read_with_ids(
        &self,
        conversation_id: &str,
        message_ids: Vec<String>,
        read_seq: u64,
    ) -> Result<()> {
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventReadReceipt as i32,
            payload: Some(EventPayload::Read(ReadReceiptEvent {
                conversation_id: conversation_id.to_string(),
                user_id,
                message_ids,
                read_seq,
                ..Default::default()
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    pub async fn typing(&self, conversation_id: &str, typing: bool) -> Result<()> {
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventTyping as i32,
            payload: Some(EventPayload::Typing(TypingEvent {
                conversation_id: conversation_id.to_string(),
                user_id,
                typing,
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    /// 按消息 ID 添加反应（内部解析会话与 server_msg_id）
    pub async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.react_impl(&conv_id, &server_id, emoji, ReactionAction::Add)
            .await
    }

    /// 按消息 ID 移除反应（内部解析会话与 server_msg_id）
    pub async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.react_impl(&conv_id, &server_id, emoji, ReactionAction::Remove)
            .await
    }

    async fn react_impl(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        emoji: &str,
        action: ReactionAction,
    ) -> Result<()> {
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventReaction as i32,
            payload: Some(EventPayload::Reaction(ReactionEvent {
                server_msg_id: server_msg_id.to_string(),
                user_id,
                emoji: emoji.to_string(),
                action: action as i32,
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    /// 置顶消息；message_id 为上层 client_msg_id，内部解析为 server_msg_id 再发服务端。
    pub async fn pin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        let (_conv_id, server_msg_id) = self.resolve_message_id(message_id).await?;
        let operator_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventPin as i32,
            payload: Some(EventPayload::Pin(PinEvent {
                server_msg_id: server_msg_id.clone(),
                pinned_by: operator_id,
                ..Default::default()
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    /// 取消置顶；message_id 为上层 client_msg_id。
    pub async fn unpin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        let (_conv_id, server_msg_id) = self.resolve_message_id(message_id).await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventUnpin as i32,
            payload: Some(EventPayload::Unpin(UnpinEvent {
                server_msg_id,
                ..Default::default()
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    pub async fn pin_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conv_id, _) = self.resolve_message_id(message_id).await?;
        self.pin(&conv_id, message_id).await
    }

    pub async fn unpin_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conv_id, _) = self.resolve_message_id(message_id).await?;
        self.unpin(&conv_id, message_id).await
    }

    /// 标记消息；message_id 为上层 client_msg_id。
    pub async fn mark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        self.mark_with_color(conversation_id, message_id, mark_type, "")
            .await
    }

    /// 带颜色的标记；message_id 为上层 client_msg_id。
    pub async fn mark_with_color(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        let (_conv_id, server_msg_id) = self.resolve_message_id(message_id).await?;
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventMark as i32,
            payload: Some(EventPayload::Mark(MarkEvent {
                server_msg_id,
                user_id,
                mark_type: mark_type as i32,
                color: color.to_string(),
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    /// 取消标记；message_id 为上层 client_msg_id。
    pub async fn unmark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        let (_conv_id, server_msg_id) = self.resolve_message_id(message_id).await?;
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventUnmark as i32,
            payload: Some(EventPayload::Unmark(UnmarkEvent {
                server_msg_id,
                user_id,
                mark_type: mark_type as i32,
            })),
            ..Default::default()
        };
        self.sender.send_event(&event, timeout()).await?;
        Ok(())
    }

    pub async fn mark_by_message_id(
        &self,
        message_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        let (conv_id, _) = self.resolve_message_id(message_id).await?;
        self.mark_with_color(&conv_id, message_id, mark_type, color)
            .await
    }

    pub async fn unmark_by_message_id(&self, message_id: &str, mark_type: MarkType) -> Result<()> {
        let (conv_id, _) = self.resolve_message_id(message_id).await?;
        self.unmark(&conv_id, message_id, mark_type).await
    }

    /// 按 message_id（上层统一为 client_msg_id）查询；先按 client_msg_id 查，再按 server_id 查。
    pub async fn get(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let msg = match self.store.get_by_client_msg_id(message_id).await? {
            Some(m) => Some(m),
            None => self.store.get(message_id).await?,
        };
        let Some(mut view) = msg else { return Ok(None) };
        if let Ok(Some(profile)) = self.profile_reader.get(&view.sender_id().to_string()).await {
            view = view.with_sender_profile(profile.display_name());
        }
        Ok(Some(view))
    }

    /// 按 message_id（上层统一为 client_msg_id）查询原始消息，不填充发送者资料。
    pub async fn get_raw(&self, message_id: &str) -> Result<Option<IMMessage>> {
        let msg = match self.store.get_by_client_msg_id(message_id).await? {
            Some(m) => Some(m),
            None => self.store.get(message_id).await?,
        };
        Ok(msg)
    }

    pub async fn list(
        &self,
        conversation_id: &str,
        before_seq: u64,
        limit: u32,
    ) -> Result<Vec<IMMessage>> {
        let list = self
            .query_handler
            .handle_get_messages(GetMessagesQuery {
                conversation_id: conversation_id.into(),
                before_seq,
                limit,
            })
            .await?;
        let mut views = Vec::with_capacity(list.len());
        for mut view in list {
            if let Ok(Some(profile)) = self.profile_reader.get(&view.sender_id().to_string()).await
            {
                view = view.with_sender_profile(profile.display_name());
            }
            views.push(view);
        }
        Ok(views)
    }

    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<IMMessage>> {
        let list = self
            .query_handler
            .handle_search_messages(SearchMessagesQuery {
                keyword: keyword.into(),
                limit,
            })
            .await?;
        let mut views = Vec::with_capacity(list.len());
        for mut view in list {
            if let Ok(Some(profile)) = self.profile_reader.get(&view.sender_id().to_string()).await
            {
                view = view.with_sender_profile(profile.display_name());
            }
            views.push(view);
        }
        Ok(views)
    }
}
