//! Flare IM Core SDK.
//!
//! This crate is the client-side source of truth for IM behavior: reliable
//! send, local pending state, sync convergence, message projections, protocol
//! mapping, and extension dispatch. Platform bindings adapt this API instead
//! of duplicating delivery or synchronization rules.

// Internal modules stay crate-private; public facades below own the SDK surface.
#[allow(dead_code, unused_imports)]
pub(crate) mod application;
pub mod client;
#[allow(dead_code, unused_imports)]
pub(crate) mod core;
#[allow(dead_code, unused_imports)]
pub(crate) mod domain;
#[allow(dead_code, unused_imports)]
pub(crate) mod extension;
#[allow(dead_code, unused_imports)]
pub(crate) mod infrastructure;
pub mod model;
#[allow(dead_code, unused_imports)]
pub(crate) mod platform;
#[allow(dead_code, unused_imports)]
pub(crate) mod shared;
pub mod spi;

/// Event subscription and SDK event payload facade.
pub mod event {
    pub use crate::core::event::*;
}

/// Storage adapter facade for core-owned persistence implementations.
///
/// Product/business extensions should prefer `spi::ExtensionStore`; this
/// module is for SDK storage adapters such as SQLite or IndexedDB.
pub mod storage {
    pub use crate::domain::{
        ConversationReader, ConversationStore, ConversationWriter, MediaCacheAdmin,
        MediaCacheEntryVo, MediaCacheStatsVo, MediaCacheStore, MessageReader, MessageStore,
        MessageWriter, PendingSendReader, PendingSendVo, PendingSendWriter, SyncCursorReader,
        SyncCursorVo, SyncCursorWriter, UserFileDownloadStore, UserReader, UserWriter,
        set_local_cleared_through_seq,
    };
    pub use crate::infrastructure::persistence::memory_im::{
        MemoryConversationStore, MemoryMessageStore,
    };
    pub use crate::infrastructure::persistence::{
        MemoryPendingSendStore, MemorySyncCursorStore, MemoryUserProfileStore, StoreProvider,
        in_memory_empty_im_provider, in_memory_im_provider,
    };
}

pub use client::*;
pub use core::SdkState;
pub use core::event::{
    CustomEventDefinition, CustomEventSelector, EventBus, EventFilter, EventReceiver,
    FilteredEventReceiver, PublishOutcome, SdkEvent, SdkEventKind, SdkEventType,
};
pub use shared::error::{ErrorCode, FlareError, Result};

/// App-facing imports for the production SDK surface.
pub mod prelude {
    pub use crate::application::{
        FileDownloadProgress, FileDownloadProgressCallback, InboundNotificationView,
        NotificationDispatchReport, NotificationHandleResult, NotificationHandler,
        NotificationHandlerRegistry, UploadPhase, UploadProgress, UploadProgressCallback,
        UserFileDownloadRequest,
    };
    pub use crate::client::{
        CapabilityApi, CapabilityDescriptorDto, CapabilityDispatchResult, ConnectedApis,
        ConversationApi, CoreTokenConfig, CreateLocationRequest, CreateRichDocRequest,
        CreateStickerRequest, EditRichDocRequest, IMClient, IMClientBuilder, LoginDbKind, MediaApi,
        MessageApi, MessageBuildApi, RtcSfuSubscriptionRequest, SdkConfig, SdkConfigBuilder,
        SdkConfigOverlay, SdkResourceProfile, SdkRuntimeResources, TransportKind, TransportPolicy,
        UserCapabilityGrantDto,
    };
    pub use crate::core::event::{
        ConnectionEvent, ConnectionEventType, ConversationEvent, ConversationEventType,
        CustomEventDefinition, CustomEventSelector, EventBus, EventFilter, EventReceiver,
        ExtensionEvent, ExtensionEventType, FilteredEventReceiver, MessageEvent, MessageEventType,
        NotificationEvent, NotificationEventType, PublishOutcome, SdkEvent, SdkEventKind,
        SdkEventType, Subscription, SyncEventType, SyncNotify, SyncPhase,
    };
    pub use crate::core::{SdkState, SyncRunContext, SyncScope, SyncTrigger, SyncVisibility};
    pub use crate::domain::{
        ConversationStore, MediaCacheEntryVo, MediaCacheStatsVo, MessageStore, PendingSendVo,
        SyncCursorVo,
    };
    pub use crate::extension::{
        ContentCodec, ConversationProjectionReport, ConversationProjectionSource, ExtensionContent,
        ExtensionContext, ExtensionLifecycle, ExtensionLifecycleContext, ExtensionMigration,
        ExtensionRegistry, ExtensionRuntime, ExtensionStore, ProfileProvider, SdkExtension,
    };
    #[cfg(feature = "storage-sqlite")]
    pub use crate::infrastructure::persistence::sqlite_init_schema;
    pub use crate::infrastructure::persistence::{
        StoreProvider, in_memory_empty_im_provider, in_memory_im_provider,
    };
    pub use crate::infrastructure::protocol::{
        Codec, DownlinkPayload, PacketSender, ProtobufCodec,
    };
    pub use crate::infrastructure::transport::{
        HttpApiResponse, HttpClient, HttpRequestContext, unwrap_api_response,
        unwrap_void_api_response,
    };
    pub use crate::model::{
        Conversation, ConversationSummary, IMMessage, MessageStatus, MessageType, SendAck,
        UploadedMedia,
    };
    pub use crate::platform::ports::media::{
        MediaMetadata, MediaServicePort, MediaSourceDescriptor, MediaSourceKind,
    };
    pub use crate::shared::error::{ErrorCode, FlareError, Result};
    #[cfg(feature = "lifecycle-sqlite")]
    pub use crate::shared::util::sqlite_store::ensure_core_sqlite_schema_registered;
    pub use crate::shared::util::{generate_core_token, resolve_user_db_path};
}
