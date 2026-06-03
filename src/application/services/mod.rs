//! Application-level support services.

pub(crate) mod event_deduper;
pub(crate) mod incoming_message_converger;
pub mod message_builder;
pub(crate) mod message_deduper;

pub(crate) use event_deduper::EventDeduper;
pub(crate) use incoming_message_converger::IncomingMessageConverger;
pub use message_builder::{
    BuildCardRequest, BuildLinkCardRequest, BuildLocationRequest, BuildMiniProgramRequest,
    BuildRichDocRequest, BuildScheduleRequest, BuildStickerRequest, MessageBuilderService,
};
pub(crate) use message_deduper::MessageDeduper;
