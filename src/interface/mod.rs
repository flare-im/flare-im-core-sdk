//! Interface Layer
//!
//! The interface layer provides the public API for the SDK, acting as a thin facade
//! over the application layer. It follows the Facade pattern to simplify complex
//! interactions and provides a clean, user-friendly API.
//!
//! ## Architecture
//!
//! The interface layer consists of:
//!
//! - **Facade**: High-level APIs for messages, conversations, and SDK lifecycle
//! - **Event**: Event subscription and handling APIs
//!
//! ## Design Principles
//!
//! 1. **Thin Layer**: The interface layer is a thin wrapper that delegates to
//!    the application layer
//! 2. **User-Friendly**: Provides convenient APIs that hide internal complexity
//! 3. **Type Safety**: Leverages Rust's type system for compile-time safety
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
//! // Login and connect
//! sdk.login("user_id".to_string(), "token".to_string()).await?;
//! sdk.connect().await?;
//!
//! // Use message facade
//! let message_facade = sdk.message();
//! // ... use message APIs
//! # Ok(())
//! # }
//! ```

pub mod facade;
pub mod event;
