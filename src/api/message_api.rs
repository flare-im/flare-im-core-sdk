use std::sync::Arc;

use crate::command::{EditMessageCommand, RecallMessageCommand, SendMessageCommand};
use crate::core::CurrentUserIdStore;
use crate::error::{SdkError, Result};
use crate::middleware::MiddlewareChain;
use crate::model::content_builder::ContentBuilder;
use crate::model::message::*;
use crate::model::message_builder::MessageBuilder;
use crate::model::content_builder::BuiltContent;
use crate::model::event::{Event, EventType};
use crate::protocol::PacketSender;
use crate::query::{GetMessagesQuery, SearchMessagesQuery};
use crate::store::MessageStore;
use flare_proto::common::event::Payload as EventPayload;

const TIMEOUT_SECS: u64 = 15;

fn timeout() -> std::time::Duration {
    std::time::Duration::from_secs(TIMEOUT_SECS)
}

/// 消息 API — 对外暴露的消息操作统一入口
///
/// 覆盖 event.proto 定义的全部消息操作：
/// - 发送 / 撤回 / 编辑 / 删除
/// - 已读回执 / 正在输入
/// - 表情反应 / 置顶取消置顶
/// - 标记取消标记
///
/// ```ignore
/// client.message().send(msg).await?;
/// client.message().recall("conv_id", "msg_id").await?;
/// client.message().typing("conv_id", "user_id", true).await?;
/// client.message().mark("conv_id", "msg_id", "user_id", MarkType::Important).await?;
/// ```
pub struct MessageApi {
    sender: Arc<PacketSender>,
    store: Arc<dyn MessageStore>,
    chain: Arc<MiddlewareChain>,
    current_user_id: CurrentUserIdStore,
}

impl MessageApi {
    pub fn new(
        sender: Arc<PacketSender>,
        store: Arc<dyn MessageStore>,
        chain: Arc<MiddlewareChain>,
        current_user_id: CurrentUserIdStore,
    ) -> Self {
        Self { sender, store, chain, current_user_id }
    }

    /// 从 SDK 获取当前用户 ID（连接后有效，未连接返回 NotConnected）
    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(SdkError::NotConnected);
        }
        Ok(uid)
    }

    // ── 发送 ─────────────────────────────────────────────

    /// 发送消息（走完整 Command 流程：拦截器 → 发送 → 本地存储）
    pub async fn send(&self, message: Message) -> Result<SendAck> {
        SendMessageCommand::new(message)
            .execute(&self.sender, self.store.as_ref(), &self.chain)
            .await
    }

    /// 发送纯文本消息（单聊可传 receiver_id，群聊不传；sender_id 从 SDK 当前用户获取）
    pub async fn send_text_message(
        &self,
        conversation_id: &str,
        text: impl AsRef<str>,
        receiver_id: Option<&str>,
    ) -> Result<SendAck> {
        let sender_id = self.current_user_id().await?;
        let text = text.as_ref();
        let msg = if let Some(rid) = receiver_id {
            MessageBuilder::new(conversation_id, &sender_id)
                .content(ContentBuilder::text(text).build())
                .receiver(rid)
                .single_chat()
                .build()?
        } else {
            MessageBuilder::text(conversation_id, &sender_id, text)?
        };
        self.send(msg).await
    }

    /// 发送引用消息（正文放 extra content_text；sender_id 从 SDK 当前用户获取）
    pub async fn send_quote_message(
        &self,
        conversation_id: &str,
        quoted_message_id: &str,
        text: impl AsRef<str>,
        preview_text: Option<&str>,
    ) -> Result<SendAck> {
        let sender_id = self.current_user_id().await?;
        let content = ContentBuilder::quote(quoted_message_id)
            .quoted_text_preview(preview_text.unwrap_or(""))
            .build();
        let msg = MessageBuilder::new(conversation_id, &sender_id)
            .content(content)
            .extra("content_text", text.as_ref())
            .build()?;
        self.send(msg).await
    }

    /// 发送线程回复（正文放 extra content_text；sender_id 从 SDK 当前用户获取）
    pub async fn send_thread_reply(
        &self,
        conversation_id: &str,
        thread_id: &str,
        text: impl AsRef<str>,
    ) -> Result<SendAck> {
        let sender_id = self.current_user_id().await?;
        let content = ContentBuilder::thread(thread_id).build();
        let msg = MessageBuilder::new(conversation_id, &sender_id)
            .content(content)
            .extra("content_text", text.as_ref())
            .build()?;
        self.send(msg).await
    }

    /// 转发消息到目标会话（sender_id 从 SDK 当前用户获取）
    pub async fn forward_message(
        &self,
        target_conversation_id: &str,
        message_ids: Vec<String>,
    ) -> Result<SendAck> {
        let sender_id = self.current_user_id().await?;
        let content = ContentBuilder::forward(message_ids).build();
        let msg = MessageBuilder::new(target_conversation_id, &sender_id)
            .content(content)
            .build()?;
        self.send(msg).await
    }

    /// 根据 message_id（client_msg_id 或 server_msg_id）解析出 (conversation_id, server_msg_id)
    async fn resolve_message_id(&self, message_id: &str) -> Result<(String, String)> {
        let msg = self
            .store
            .get(message_id)
            .await?
            .ok_or_else(|| crate::error::SdkError::NotFound(format!("message not found: {}", message_id)))?;
        let server_id = if msg.server_id.is_empty() {
            msg.client_msg_id.clone()
        } else {
            msg.server_id.clone()
        };
        Ok((msg.conversation_id.clone(), server_id))
    }

    // ── 撤回 ─────────────────────────────────────────────

    /// 撤回消息（EVENT_MESSAGE_RECALL）
    pub async fn recall(&self, conversation_id: &str, server_msg_id: &str) -> Result<()> {
        RecallMessageCommand {
            conversation_id: conversation_id.to_string(),
            server_msg_id: server_msg_id.to_string(),
        }
        .execute(&self.sender, self.store.as_ref())
        .await
    }

    /// 按 message_id 撤回（SDK 内解析会话与 server_msg_id，上层统一逻辑）
    pub async fn recall_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.recall(&conv_id, &server_id).await
    }

    // ── 编辑 ─────────────────────────────────────────────

    /// 编辑消息 — 接受原始 bytes（EVENT_MESSAGE_EDIT）
    pub async fn edit(&self, conversation_id: &str, server_msg_id: &str, new_content: Vec<u8>) -> Result<()> {
        EditMessageCommand {
            conversation_id: conversation_id.to_string(),
            server_msg_id: server_msg_id.to_string(),
            new_content,
        }
        .execute(&self.sender)
        .await
    }

    /// 编辑消息 — 接受 BuiltContent（类型安全）
    pub async fn edit_content(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        content: BuiltContent,
    ) -> Result<()> {
        self.edit(conversation_id, server_msg_id, content.encode()).await
    }

    /// 按 message_id 编辑文本（SDK 内解析 + 构建 Content）
    pub async fn edit_text_by_message_id(&self, message_id: &str, text: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.edit_content(&conv_id, &server_id, ContentBuilder::text(text).build())
            .await
    }

    // ── 删除 ─────────────────────────────────────────────

    /// 删除消息（EVENT_MESSAGE_DELETE）
    pub async fn delete(&self, conversation_id: &str, server_msg_id: &str) -> Result<()> {
        self.delete_with_options(conversation_id, server_msg_id, None, None, None).await
    }

    /// 按 message_id 删除
    pub async fn delete_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.delete(&conv_id, &server_id).await
    }

    /// 删除消息（完整参数）
    pub async fn delete_with_options(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        delete_type: Option<DeleteType>,
        scope: Option<DeleteScope>,
        reason: Option<String>,
    ) -> Result<()> {
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventMessageDelete as i32,
            payload: Some(EventPayload::Delete(MessageDeleteEvent {
                server_msg_id: server_msg_id.to_string(),
                delete_type: delete_type.map(|d| d as i32),
                scope: scope.map(|s| s as i32),
                reason,
                ..Default::default()
            })),
            ..Default::default()
        };
        self.sender.send_event(event, timeout()).await?;
        self.store.delete(server_msg_id).await?;
        Ok(())
    }

    // ── 已读回执 ─────────────────────────────────────────

    /// 发送已读回执（EVENT_READ_RECEIPT；user_id 从 SDK 当前用户获取）
    pub async fn mark_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.mark_read_with_ids(conversation_id, Vec::new(), read_seq).await
    }

    /// 发送已读回执（指定具体消息 ID；user_id 从 SDK 当前用户获取）
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
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    // ── 正在输入 ─────────────────────────────────────────

    /// 发送正在输入状态（EVENT_TYPING，fire-and-forget；user_id 从 SDK 当前用户获取）
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
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    // ── 表情反应 ─────────────────────────────────────────

    /// 添加表情反应（EVENT_REACTION；user_id 从 SDK 当前用户获取）
    pub async fn add_reaction(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        emoji: &str,
    ) -> Result<()> {
        self.react(conversation_id, server_msg_id, emoji, ReactionAction::Add).await
    }

    /// 移除表情反应（EVENT_REACTION；user_id 从 SDK 当前用户获取）
    pub async fn remove_reaction(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        emoji: &str,
    ) -> Result<()> {
        self.react(conversation_id, server_msg_id, emoji, ReactionAction::Remove).await
    }

    /// 按 message_id 添加反应（user_id 从 SDK 当前用户获取）
    pub async fn add_reaction_by_message_id(&self, message_id: &str, emoji: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.add_reaction(&conv_id, &server_id, emoji).await
    }

    /// 按 message_id 移除反应（user_id 从 SDK 当前用户获取）
    pub async fn remove_reaction_by_message_id(&self, message_id: &str, emoji: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.remove_reaction(&conv_id, &server_id, emoji).await
    }

    /// 表情反应（EVENT_REACTION；user_id 从 SDK 当前用户获取）
    pub async fn react(
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
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    // ── 置顶 ─────────────────────────────────────────────

    /// 置顶消息（EVENT_PIN；operator_id 从 SDK 当前用户获取）
    pub async fn pin(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
    ) -> Result<()> {
        let operator_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventPin as i32,
            payload: Some(EventPayload::Pin(PinEvent {
                server_msg_id: server_msg_id.to_string(),
                pinned_by: operator_id,
                ..Default::default()
            })),
            ..Default::default()
        };
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    /// 取消置顶（EVENT_UNPIN）
    pub async fn unpin(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
    ) -> Result<()> {
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventUnpin as i32,
            payload: Some(EventPayload::Unpin(UnpinEvent {
                server_msg_id: server_msg_id.to_string(),
            })),
            ..Default::default()
        };
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    /// 按 message_id 置顶（operator_id 从 SDK 当前用户获取）
    pub async fn pin_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.pin(&conv_id, &server_id).await
    }

    /// 按 message_id 取消置顶
    pub async fn unpin_by_message_id(&self, message_id: &str) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.unpin(&conv_id, &server_id).await
    }

    // ── 标记 ─────────────────────────────────────────────

    /// 标记消息（EVENT_MARK）— 重要/待办/已处理/自定义（user_id 从 SDK 当前用户获取）
    pub async fn mark(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        self.mark_with_color(conversation_id, server_msg_id, mark_type, "").await
    }

    /// 标记消息（带自定义颜色；user_id 从 SDK 当前用户获取）
    pub async fn mark_with_color(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventMark as i32,
            payload: Some(EventPayload::Mark(MarkEvent {
                server_msg_id: server_msg_id.to_string(),
                user_id,
                mark_type: mark_type as i32,
                color: color.to_string(),
            })),
            ..Default::default()
        };
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    /// 取消标记（EVENT_UNMARK；user_id 从 SDK 当前用户获取）
    pub async fn unmark(
        &self,
        conversation_id: &str,
        server_msg_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        let user_id = self.current_user_id().await?;
        let event = Event {
            conversation_id: conversation_id.to_string(),
            r#type: EventType::EventUnmark as i32,
            payload: Some(EventPayload::Unmark(UnmarkEvent {
                server_msg_id: server_msg_id.to_string(),
                user_id,
                mark_type: mark_type as i32,
            })),
            ..Default::default()
        };
        self.sender.send_event(event, timeout()).await?;
        Ok(())
    }

    /// 按 message_id 标记（重要/待办等；user_id 从 SDK 当前用户获取）
    pub async fn mark_by_message_id(
        &self,
        message_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.mark_with_color(&conv_id, &server_id, mark_type, color).await
    }

    /// 按 message_id 取消标记（user_id 从 SDK 当前用户获取）
    pub async fn unmark_by_message_id(&self, message_id: &str, mark_type: MarkType) -> Result<()> {
        let (conv_id, server_id) = self.resolve_message_id(message_id).await?;
        self.unmark(&conv_id, &server_id, mark_type).await
    }

    // ── 查询 (Query) ────────────────────────────────────

    /// 获取单条消息
    pub async fn get(&self, message_id: &str) -> Result<Option<Message>> {
        self.store.get(message_id).await
    }

    /// 查询会话消息列表（按 seq 倒序）
    pub async fn list(&self, conversation_id: &str, before_seq: u64, limit: u32) -> Result<Vec<Message>> {
        GetMessagesQuery { conversation_id: conversation_id.into(), before_seq, limit }
            .execute(self.store.as_ref()).await
    }

    /// 搜索消息
    pub async fn search(&self, keyword: &str, limit: u32) -> Result<Vec<Message>> {
        SearchMessagesQuery { keyword: keyword.into(), limit }
            .execute(self.store.as_ref()).await
    }
}
