//! 共享模块
//!
//! 跨层共享的通用功能

pub mod capability;
pub mod config;
pub mod error;
#[cfg(feature = "extensions")]
pub mod extension;
pub mod memory_leak_detector;
pub mod metrics;
pub mod observer;
pub mod platform;
pub mod utils;
pub mod validation;

pub use capability::*;
pub use config::*;
pub use error::*;
#[cfg(feature = "extensions")]
pub use extension::*;
pub use memory_leak_detector::*;
pub use metrics::*;
pub use observer::*;
pub use platform::*;
pub use utils::*;
pub use validation::*;
