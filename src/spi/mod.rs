//! Stable extension SPI for business SDKs and optional plugins.
//!
//! Extensions should depend on this module plus `model`, `event`, and
//! app-facing `prelude` types instead of reaching into application, domain,
//! infrastructure, or storage internals.

pub use crate::application::{
    InboundNotificationView, LocalConversationClearResult, LocalConversationVisibility,
    NotificationDispatchReport, NotificationHandleResult, NotificationHandler,
};
pub use crate::core::{
    SyncContext, SyncFailurePolicy, SyncMode, SyncResult, SyncTask, SyncTaskResult,
};
pub use crate::domain::UserProfile;
pub use crate::domain::conversation::id::{
    CidConversationType, extract_conversation_type, generate_ai_conversation_id,
    generate_customer_conversation_id, generate_system_conversation_id,
    generate_temp_conversation_id, is_group_chat_conversation, is_single_chat_conversation,
    validate_conversation_id,
};
pub use crate::extension::capability::{SdkCapabilityPlugin, SdkCapabilityRegistry};
pub use crate::extension::middleware::{EventInterceptor, MessageInterceptor};
pub use crate::extension::{
    ContentCodec, ConversationProjectionReport, ConversationProjectionSource, ExtensionContent,
    ExtensionContext, ExtensionLifecycle, ExtensionLifecycleContext, ExtensionMigration,
    ExtensionRegistry, ExtensionRuntime, ExtensionStore, ProfileProvider, SdkExtension,
    generate_group_conversation_id, generate_single_chat_conversation_id,
    merge_or_create_conversation, repair_single_chat_channel,
};
pub use crate::model::conversation::ConversationType;
pub use crate::model::{Conversation, IMMessage};
