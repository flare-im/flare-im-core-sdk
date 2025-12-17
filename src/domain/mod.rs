#[cfg(feature = "extensions")]
pub mod extension;
pub mod message;
pub mod message_builder;
pub mod session;
pub mod sync;

// 重新导出消息领域类型
pub use message::event::{
    MessageDeletedEvent, MessageEditedEvent, MessageFavoritedEvent, MessageForwardedEvent,
    MessagePinnedEvent, MessageReactionAddedEvent, MessageReactionRemovedEvent, MessageReadEvent,
    MessageRecalledEvent, MessageReceivedEvent, MessageSentEvent, MessageUnfavoritedEvent,
    MessageUnpinnedEvent,
};
pub use message::model::{Message as DomainMessage, MessageError, MessageId, SessionId, UserId};
pub use message::repository::MessageRepository;
pub use message::service::MessageDomainService;

// 重新导出会话领域类型
pub use session::event::{
    SessionCreatedEvent, SessionDeletedEvent, SessionDraftSetEvent, SessionHiddenEvent,
    SessionMarkedReadEvent, SessionShownEvent, SessionTypingSentEvent, SessionUpdatedEvent,
};
pub use session::model::{Session as DomainSession, SessionError};
pub use session::repository::SessionRepository;
pub use session::service::SessionDomainService;

// 重新导出同步领域类型
pub use sync::event::{SyncCompletedEvent, SyncFailedEvent, SyncStartedEvent};
pub use sync::model::{Sync as DomainSync, SyncError, SyncStatus, SyncType};
pub use sync::repository::SyncRepository;
pub use sync::service::SyncDomainService;

// 重新导出 flare_proto 类型（用于领域服务接口）
pub use flare_proto::{MessageContent, MessageType};

#[cfg(feature = "extensions")]
pub use extension::{
    DefaultExtensionProvider, ExtensionCache, ExtensionProvider, MessageExtension,
    MessageLocalState, SessionExtension, UserExtension,
};
// ExtendedMessage 已删除，使用 DomainMessage + Extension 替代
// #[cfg(feature = "extensions")]
// pub use message::ExtendedMessage;

// 重新导出 flare_proto::Message（用于与外部交互）
pub use flare_proto::Message;

// Domain Message 聚合根（已在上面的 use 中导出为 DomainMessage）
pub use message_builder::MessageBuilder;

// 重新导出领域服务实现
pub use message::service::MessageDomainServiceImpl;
pub use session::service::SessionDomainServiceImpl;
pub use sync::service::SyncDomainServiceImpl;

#[cfg(feature = "extensions")]
pub use session::ExtendedSessionSummary;

pub use session::{SessionSummary, SessionSummaryProto};

pub use sync::{SyncCursor, SyncResult};
