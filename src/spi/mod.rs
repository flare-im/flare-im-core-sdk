//! Stable extension SPI for business SDKs and optional plugins.
//!
//! Extensions should depend on this module plus `model`, `event`, and
//! app-facing `prelude` types instead of reaching into application, domain,
//! infrastructure, or storage internals.

pub mod metrics;

pub use crate::application::{
    InboundNotificationView, LocalConversationClearResult, LocalConversationVisibility,
    NotificationDispatchReport, NotificationHandleResult, NotificationHandler,
};
pub use crate::domain::UserProfile;
pub use crate::domain::conversation::id::{
    CidConversationType, extract_conversation_type, generate_ai_conversation_id,
    generate_customer_conversation_id, generate_system_conversation_id,
    generate_temp_conversation_id, is_group_chat_conversation, is_single_chat_conversation,
    validate_conversation_id,
};
pub use crate::extension::capability::{SdkCapabilityPlugin, SdkCapabilityRegistry};
pub use crate::extension::encryption::{
    ContentEncryptionInterceptor, ConversationEncryptionPolicy,
    ConversationEncryptionPolicyResolver, E2EE_CONTENT_TYPE, E2EE_FALLBACK_TEXT,
    E2EE_PLACEHOLDER_REASON, E2eeIdentityKey, E2eeKeyManager, E2eePreKeyBundle,
    E2eeSessionDescriptor, EncryptedContentEnvelope, EncryptionTier,
    KeyManagedConversationEncryptionPolicyResolver, PLAINTEXT_CONTENT_TYPE,
    StaticConversationEncryptionPolicyResolver, VolatileE2eeKeyManager, encrypted_content_envelope,
    encrypted_content_envelope_from_bytes,
};
pub use crate::extension::middleware::{EventInterceptor, MessageInterceptor};
pub use crate::extension::{
    ContentCodec, ConversationProjectionReport, ConversationProjectionSource, ExtensionContent,
    ExtensionContext, ExtensionLifecycle, ExtensionLifecycleContext, ExtensionMigration,
    ExtensionRegistry, ExtensionRuntime, ExtensionStore, ProfileProvider, SdkExtension,
    generate_group_conversation_id, generate_single_chat_conversation_id,
    merge_or_create_conversation, repair_single_chat_channel,
};
pub use crate::kernel::{
    ApplyOutcome, ConvergencePriority, DomainCursor, DomainDelta, DomainId, DomainItem,
    DomainPhase, LaneSpec, SyncContext, SyncDomain, SyncFailurePolicy, SyncMode, SyncResult,
    SyncTask, SyncTaskResult,
};
pub use crate::model::conversation::ConversationType;
pub use crate::model::{Conversation, IMMessage};
pub use crate::platform::ports::storage::{
    LOCAL_DATABASE_KEY_BYTES, SecureKeyDescriptor, SecureKeyStore, SecureSecret,
    VolatileSecureKeyStore, load_or_create_local_database_key, validate_local_database_key,
};
pub use metrics::{
    HistogramSnapshot, InMemoryMetricsSink, MetricLabel, MetricsRecorder, MetricsSink,
    MetricsSnapshot,
};
