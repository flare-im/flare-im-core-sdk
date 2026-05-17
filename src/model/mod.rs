//! 协议层模型 — 与 flare_proto 对齐，并扩展 ContentBuilder / MessageBuilder / DecodedContent

pub mod content_builder;
pub mod conversation;
pub mod decoder;
pub mod event;
pub mod media;
pub mod message;
pub mod message_builder;
pub mod message_elem;
pub mod preview_storage;

pub use crate::domain::{MediaCacheEntryVo, MediaCacheStatsVo};
pub use content_builder::{BuiltContent, ContentBuilder, DEFAULT_STICKER_DISPLAY_SIDE};
pub use conversation::{Conversation, ConversationParticipant, ConversationSummary};
pub use decoder::{DecodedContent, decode_content, decode_content_bytes};
pub use event::{Event, EventType};
pub use media::{MediaAccessUrl, MediaResolvedAccess, UploadOptions, UploadedMedia};
pub use message::{
    ConversationType, DeleteScope, DeleteType, IMMessage, MarkType, Message, MessageSource,
    MessageStatus, MessageType, ReactionAction, SendAck,
};
pub use message_builder::MessageBuilder;
pub use message_elem::{Elem, MessagePreviewElem, decoded_content_to_elem};
pub use preview_storage::PreviewStoragePayload;
