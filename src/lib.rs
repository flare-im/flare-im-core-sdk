//! # Flare IM Core SDK
//!
//! Production-grade, event-driven, cross-platform IM core SDK.
//!
//! This crate owns only the business-neutral IM core:
//! messages, conversations, connection lifecycle, reliable sending, local
//! storage coordination, and offline/incremental sync. Media, storage,
//! transport, crypto, and runtime differences are injected through
//! `platform`; product capabilities are installed through `extension`.
//!
//! ## Public surfaces
//!
//! - `client`: application-facing SDK handle, lifecycle, builder, and APIs.
//! - `prelude`: app-facing API imports for normal SDK usage.
//! - `adapter_prelude`: storage/protocol/platform contracts for host adapters.
//! - `extension_prelude`: plugin and business-extension contracts.
//! - `domain`, `application`, and `core`: IM core layers.
//! - `platform` and `infrastructure`: runtime boundary and concrete IO.
//! - `extension`: business extension, capability plugin, and middleware contracts.

// Public API layer.
pub mod client;

// Core business layers.
pub mod application;
pub mod core;
pub mod domain;
pub mod extension;
pub mod model;

// Runtime boundaries and shared primitives.
pub mod infrastructure;
pub mod platform;
pub mod shared;

/// 通话 / RTC 插件 crate 再导出（需启用 **`plugin-call`** feature）。
#[cfg(feature = "plugin-call")]
pub use flare_sdk_plugin_call;
/// 通话插件 **生产 API 边界**（[`flare_sdk_plugin_call::production`]）。
#[cfg(feature = "plugin-call")]
pub use flare_sdk_plugin_call::production as call_plugin;

/// 错误类型与 Result 根级导出（与 flare-core 一致，便于 bindings 等使用）
pub use shared::error::{ErrorCode, FlareError, Result, from_rpc_status};
/// 强类型 ID（防止 user_id / conversation_id 混用）
pub use shared::types::{ConversationId, UserId};

/// App-facing imports for normal SDK consumers.
pub mod prelude {
    pub use crate::client::{
        CapabilityApi, CapabilityDescriptorDto, CapabilityDispatchResult, ConversationApi,
        FileDownloadProgress, FileDownloadProgressCallback, IMClient, IMClientBuilder,
        MediaAccessUrl, MediaApi, MediaCacheEntryVo, MediaCacheStatsVo, MediaResolvedAccess,
        MessageApi, MessageBuildApi, SdkConfig, SdkConfigBuilder, UploadOptions, UploadPhase,
        UploadProgress, UploadProgressCallback, UploadedMedia, UserCapabilityGrantDto,
    };
    pub use crate::core::SdkState;
    #[cfg(feature = "plugin-call")]
    pub use crate::extension::capability::{
        AvExperienceSpec, CallControlSet, CallLayoutMode, ExperienceEdition,
        default_call_experience_spec, sanitize_experience_spec_for_edition,
    };
    pub use crate::shared::error::{ErrorCode, FlareError, Result, from_rpc_status};
    pub use crate::shared::types::{ConversationId, UserId};

    pub use crate::core::event::{ConversationEvent, MessageEvent, NotificationEvent};
    pub use crate::core::event::{EventBus, EventReceiver, SdkEvent, Subscription};

    pub use crate::application::notification::{
        InboundNotificationView, NotificationHandleResult, NotificationHandler,
        NotificationHandlerRegistry,
    };
    pub use crate::client::profile_center::{
        ProfileCenterAction, ProfileCenterActionKind, ProfileCenterContract, ProfileCenterSummary,
        default_profile_center_actions,
    };

    pub use crate::core::SyncState;
    pub use crate::core::{
        SyncFailurePolicy, SyncMode, SyncPhase, SyncProgress, SyncReason, SyncRunContext,
        SyncScope, SyncTrigger, SyncVisibility,
    };

    pub use crate::domain::conversation::id::generate_single_chat_conversation_id;

    pub use crate::model::{BuiltContent, ContentBuilder, MessageBuilder};
    pub use crate::model::{DecodedContent, decode_content, decode_content_bytes};
    pub use crate::model::{Elem, decoded_content_to_elem};

    pub use crate::model::message::{
        ConversationType, DeleteScope, DeleteType, MarkType, Message, MessageSource, MessageStatus,
        MessageType, ReactionAction,
    };

    pub use crate::domain::UserProfile;
    pub use crate::model::{
        Conversation, ConversationListQuery, IMMessage, MessageSearchKind, MessageSearchQuery,
    };
}

/// Host-adapter imports for platform SDKs and bindings.
pub mod adapter_prelude {
    pub use crate::domain::{ConversationStore, MessageStore, SyncCursorStore};
    pub use crate::infrastructure::persistence::StoreProvider;
    pub use crate::infrastructure::protocol::{Codec, ProtobufCodec};
    pub use crate::platform::adapters::{
        AdapterPlatform, AdapterProvisioning, MediaAdapterProfile, MediaSourceSupport,
        PlatformAdapterProfile, StorageAdapterProfile, UploadOnlyMediaService,
    };
    pub use crate::platform::ports::media::{
        MediaMetadata, MediaServicePort, MediaSourceDescriptor, MediaSourceKind, MediaUploaderPort,
        ProcessedMedia, UploadProgressSink,
    };
    pub use crate::platform::runtime::{
        MediaRuntimeConfig, MediaRuntimeKind, NativeRuntimeAssembler, PlatformKind,
        RuntimeAssembler, RuntimeComponents, RuntimeConfig, StorageConfig, StorageKind,
    };
}

/// Extension-author imports for business SDKs such as `flare-social-sdk`.
pub mod extension_prelude {
    pub use crate::core::{
        SyncContext, SyncFailurePolicy, SyncMode, SyncPhase, SyncProgress, SyncReason,
        SyncRunContext, SyncScope, SyncTask, SyncTaskResult, SyncTrigger, SyncVisibility,
    };
    #[cfg(feature = "plugin-call")]
    pub use crate::extension::capability::AvCapabilityPlugin;
    pub use crate::extension::capability::{SdkCapabilityPlugin, SdkCapabilityRegistry};
    pub use crate::extension::middleware::{
        EventInterceptor, EventMiddlewareAction, MessageInterceptor, MessageMiddlewareContext,
        MessageOperation,
    };
    pub use crate::extension::{ExtensionRegistry, SdkExtension};
    pub use crate::shared::error::{ErrorCode, FlareError, Result};
}
