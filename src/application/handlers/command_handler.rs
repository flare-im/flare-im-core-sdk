//! 主命令处理器（编排层）
//!
//! 职责：分发命令到具体的处理器，只负责编排，不包含业务逻辑
//! 
//! 架构模式：CQRS Command Handler
//! - 接收 Command 对象
//! - 委托给领域服务执行业务逻辑
//! - 确保事务边界和一致性

use std::sync::Arc;
use tokio::sync::Mutex;
use crate::application::fsm::FsmManager;
use crate::domain::repository::{EventStore, MessageRepository, ConversationRepository};
use crate::infrastructure::network::NetworkClient;
use crate::infrastructure::storage::media_cache::MediaCacheManager;
use crate::infrastructure::messaging::MessageSender;
use crate::config::SdkConfig;
use crate::infrastructure::event_bus::EventBus;
use crate::application::extension::ExtensionRegistry;
use crate::domain::message_queue::MessageQueue;

use super::{
    MessageCommandHandler,
    ConversationCommandHandler,
    SessionCommandHandler,
};
use crate::application::commands::*;

/// 主命令处理器
///
/// 职责：编排所有写操作，分发命令到具体的处理器
/// 
/// 架构设计：
/// - 统一管理用户上下文（从 FSM 获取当前用户 ID）
/// - Facade 层不需要传入 user_id，由 Application 层自动填充
/// - 遵循单一职责原则：Facade 关注 API 简洁性，Application 关注上下文管理
/// - 实现 CQRS 模式的 Command Handler 层
pub struct CommandHandler {
    message_handler: Arc<MessageCommandHandler>,
    conversation_handler: Arc<ConversationCommandHandler>,
    session_handler: Arc<SessionCommandHandler>,
}

impl CommandHandler {
    pub fn new(
        fsm: Arc<FsmManager>,
        event_store: Arc<dyn EventStore>,
        message_repository: Arc<dyn MessageRepository>,
        conversation_repository: Arc<dyn ConversationRepository>,
        config: SdkConfig,
        media_cache: Arc<MediaCacheManager>,
        event_bus: Arc<EventBus>,
        extension_registry: Arc<ExtensionRegistry>,
        message_queue: Option<Arc<MessageQueue>>,
        network: Arc<Mutex<Option<NetworkClient>>>,
        message_sender: Arc<MessageSender>,
    ) -> anyhow::Result<Self> {
        let message_handler = Arc::new(MessageCommandHandler::new(
            fsm.clone(),
            event_store.clone(),
            message_repository.clone(),
            message_sender,
            media_cache,
            event_bus.clone(),
        ));
        
        let conversation_handler = Arc::new(ConversationCommandHandler::new(
            fsm.clone(),
            event_store.clone(),
            conversation_repository.clone(),
        ));
        
        let session_handler = Arc::new(SessionCommandHandler::new(
            fsm.clone(),
            conversation_repository.clone(),
            config,
            network,
            event_bus,
            extension_registry,
            message_queue,
        ));
        
        Ok(Self {
            message_handler,
            conversation_handler,
            session_handler,
        })
    }
    
    
    /// 设置网络客户端
    pub async fn set_network_client(&self, client: NetworkClient) {
        self.session_handler.set_network_client(client).await;
    }
    
    // ============================================================================
    // 消息命令（委托给 MessageCommandHandler）
    // ============================================================================
    
    pub async fn send_message(&self, cmd: SendMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle(cmd).await
    }
    
    pub async fn recall_message(&self, cmd: RecallMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_recall(cmd).await
    }

    pub async fn edit_message(&self, cmd: EditMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_edit(cmd).await
    }

    pub async fn delete_message(&self, cmd: DeleteMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_delete(cmd).await
    }
    
    pub async fn mark_messages_read(&self, cmd: MarkMessagesReadCommand) -> anyhow::Result<()> {
        self.message_handler.handle_mark_read(cmd).await
    }
    
    pub async fn reply_message(&self, cmd: ReplyMessageCommand) -> anyhow::Result<String> {
        self.message_handler.handle_reply(cmd).await
    }
    
    pub async fn forward_messages(&self, cmd: ForwardMessagesCommand) -> anyhow::Result<Vec<String>> {
        self.message_handler.handle_forward(cmd).await
    }
    
    pub async fn add_reaction(&self, cmd: AddReactionCommand) -> anyhow::Result<()> {
        self.message_handler.handle_add_reaction(cmd).await
    }
    
    pub async fn remove_reaction(&self, cmd: RemoveReactionCommand) -> anyhow::Result<()> {
        self.message_handler.handle_remove_reaction(cmd).await
    }
    
    
    pub async fn pin_message(&self, cmd: PinMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_pin(cmd).await
    }
    
    pub async fn unpin_message(&self, cmd: UnpinMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_unpin(cmd).await
    }
    
    pub async fn favorite_message(&self, cmd: FavoriteMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_favorite(cmd).await
    }
    
    pub async fn unfavorite_message(&self, cmd: UnfavoriteMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_unfavorite(cmd).await
    }
    
    pub async fn mark_message(&self, cmd: MarkMessageCommand) -> anyhow::Result<()> {
        self.message_handler.handle_mark(cmd).await
    }
    
    pub async fn add_thread_reply(&self, cmd: crate::application::commands::AddThreadReplyCommand) -> anyhow::Result<String> {
        self.message_handler.handle_thread_reply(cmd).await
    }
    
    pub async fn batch_mark_message_read(&self, cmd: crate::application::commands::BatchMarkMessageReadCommand) -> anyhow::Result<()> {
        self.message_handler.handle_batch_mark_read(cmd).await
    }
    
    // ============================================================================
    // 会话命令（委托给 ConversationCommandHandler）
    // 
    // 架构设计：Application 层统一管理用户上下文
    // - Command 对象只包含业务参数，不包含用户身份信息
    // - Handler 从 FSM 获取当前用户 ID，符合单一职责原则
    // - Facade 层不需要传入 user_id，保持 API 简洁
    // ============================================================================
    
    pub async fn mark_conversation_read(&self, cmd: MarkConversationReadCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_mark_read(cmd).await
    }
    
    pub async fn mark_all_conversations_read(&self, cmd: MarkAllConversationsReadCommand) -> anyhow::Result<usize> {
        self.conversation_handler.handle_mark_all_read(cmd).await
    }
    
    pub async fn set_conversation_draft(&self, cmd: SetConversationDraftCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_set_draft(cmd).await
    }
    
    pub async fn clear_conversation_draft(&self, cmd: ClearConversationDraftCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_clear_draft(cmd).await
    }
    
    pub async fn pin_conversation(&self, cmd: PinConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_pin(cmd).await
    }
    
    pub async fn unpin_conversation(&self, cmd: UnpinConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_unpin(cmd).await
    }
    
    pub async fn mute_conversation(&self, cmd: MuteConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_mute(cmd).await
    }
    
    pub async fn unmute_conversation(&self, cmd: UnmuteConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_unmute(cmd).await
    }
    
    pub async fn set_input_state(&self, cmd: SetInputStateCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_set_input_state(cmd).await
    }
    
    pub async fn clear_input_state(&self, cmd: ClearInputStateCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_clear_input_state(cmd).await
    }
    
    pub async fn delete_conversation(&self, cmd: DeleteConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_delete(cmd).await
    }
    
    pub async fn hide_conversation(&self, cmd: HideConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_hide(cmd).await
    }
    
    pub async fn hide_all_conversations(&self, cmd: HideAllConversationCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_hide_all(cmd).await
    }
    
    pub async fn clear_conversation_messages(&self, cmd: ClearConversationMessagesCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_clear_messages(cmd).await
    }
    
    pub async fn set_conversation_info(&self, cmd: SetConversationInfoCommand) -> anyhow::Result<()> {
        self.conversation_handler.handle_set_info(cmd).await
    }
    
    // ============================================================================
    // 会话命令（委托给 SessionCommandHandler）
    // ============================================================================
    
    pub async fn login(&self, cmd: LoginCommand) -> anyhow::Result<()> {
        self.session_handler.handle_login(cmd).await
    }
    
    pub async fn logout(&self, cmd: LogoutCommand) -> anyhow::Result<()> {
        self.session_handler.handle_logout(cmd).await
    }
    
    pub async fn connect(&self, cmd: ConnectCommand) -> anyhow::Result<()> {
        self.session_handler.handle_connect(cmd).await
    }
    
    pub async fn disconnect(&self, cmd: DisconnectCommand) -> anyhow::Result<()> {
        self.session_handler.handle_disconnect(cmd).await
    }
    
    // ============================================================================
    // 向后兼容的便捷方法（直接传递参数，内部转换为 Command）
    // ============================================================================
    
    /// 发送消息（便捷方法）
    pub async fn send_message_direct(&self, message: crate::domain::message::Message) -> anyhow::Result<()> {
        self.send_message(SendMessageCommand { message }).await
    }
    
    /// 登录（便捷方法）
    pub async fn login_direct(&self, user_id: String, token: String) -> anyhow::Result<()> {
        self.login(LoginCommand { user_id, token }).await
    }
    
    /// 登出（便捷方法）
    pub async fn logout_direct(&self) -> anyhow::Result<()> {
        self.logout(LogoutCommand).await
    }
    
    /// 连接（便捷方法）
    pub async fn connect_direct(&self) -> anyhow::Result<()> {
        self.connect(ConnectCommand).await
    }
}
