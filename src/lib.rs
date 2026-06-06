//! Flare IM Core SDK vNext.
//!
//! This crate is the single source of truth for client IM behavior.  Platform
//! bindings translate ABI or IPC into this API; they do not duplicate delivery,
//! sync, message, presence, media, or capability rules.
//!
//! Boundary:
//!
//! - `flare-core` owns transport frames, connection negotiation, heartbeat, and
//!   protocol-racing concerns.
//! - `flare-proto` owns typed IM wire contracts.
//! - this crate owns offline-first client state convergence, local pending
//!   state, event publication, protocol mapping, and extension routing.

mod sdk;

pub use sdk::*;

/// Compatibility-free app-facing imports for the vNext SDK.
pub mod prelude {
    pub use crate::{
        CapabilityPacketRequest, Conversation, DownlinkPayload, ErrorCode, EventBus, EventReceiver,
        FlareError, IMClient, ImMessage, InMemoryStore, LocalStoreSnapshot, OutboundPacket,
        ProtocolCodec, Result, SdkConfig, SdkConfigBuilder, SdkEvent, SdkState, TextMessageRequest,
        TransportKind, TransportPort,
    };
}

/// Minimal shared namespace kept for binding crates.
pub mod shared {
    pub mod error {
        pub use crate::{ErrorCode, FlareError, Result};
    }

    pub mod util {
        pub use crate::generate_test_token;
    }
}

/// Core namespace kept intentionally small.  It mirrors the new SDK state and
/// event vocabulary, not the removed prototype-era module tree.
pub mod core {
    pub use crate::{EventBus, EventReceiver, SdkEvent, SdkState};
}
