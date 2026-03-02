//! Facade Module
//!
//! Provides high-level facades for different aspects of the SDK:
//!
//! - [`ImCoreSdk`]: Main SDK entry point
//! - [`MessageFacade`]: Message-related operations
//! - [`ConversationFacade`]: Conversation management
//! - [`EventSubscriptionFacade`]: Event subscription APIs
//! - [`DefaultMessageHandler`]: Default message handler implementation
//!
//! ## Design Pattern
//!
//! The facade pattern is used to provide a simplified interface to a complex
//! subsystem. Each facade encapsulates the complexity of the underlying layers
//! and provides a clean, easy-to-use API.
//!
//! ## Example
//!
//! ```no_run
//! use flare_im_core_sdk::interface::facade::ImCoreSdk;
//! use flare_im_core_sdk::config::SdkConfig;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = SdkConfig::default();
//! let sdk = ImCoreSdk::new(config).await?;
//!
//! // Access facades
//! let message_facade = sdk.message();
//! let conversation_facade = sdk.conversation();
//! let event_facade = sdk.events();
//! # Ok(())
//! # }
//! ```

pub mod facade;
pub mod message_facade;
pub mod conversation_facade;
pub mod event_subscription_facade;
pub mod default_message_handler;

pub use facade::ImCoreSdk;
pub use message_facade::MessageFacade;
pub use conversation_facade::ConversationFacade;
pub use event_subscription_facade::EventSubscriptionFacade;
pub use default_message_handler::DefaultMessageHandler;

/// Re-export mention-related types from domain service
pub use crate::domain::service::{MentionInfo, MentionInfoType};
