//! Local application lifecycle operations.

pub mod conversation_local;

pub use conversation_local::{
    ConversationLocalLifecycle, LocalConversationClearResult, LocalConversationVisibility,
};
