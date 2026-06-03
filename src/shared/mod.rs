//! Shared SDK primitives.
//!
//! This module keeps cross-cutting primitives out of the business and platform
//! layers: error contracts, strongly typed IDs, configuration helpers, and
//! low-level utilities.

pub mod config;
pub mod error;
pub mod types;
pub mod util;
