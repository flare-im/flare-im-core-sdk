//! Local read-model projection services.

pub mod conversation_display;
pub(crate) mod conversation_projection;
pub mod user_profile;

pub use conversation_display::{
    ConversationDisplayProjectionApplier, ConversationDisplaySnapshot, resolve_display_name,
};
pub(crate) use conversation_projection::ConversationProjectionApplier;
pub use user_profile::UserProfileProjectionApplier;
