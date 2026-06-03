//! Core SDK ports.
//!
//! Ports are the platform boundary of the SDK. Domain and application code
//! should depend on these contracts, while Web, native, RN, uni-app, HarmonyOS,
//! and Electron provide concrete adapters.

pub mod crypto;
pub mod media;
pub mod runtime;
pub mod storage;
pub mod transport;
