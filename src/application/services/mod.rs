//! Application-level support services.

pub(crate) mod event_deduper;
pub(crate) mod incoming_message_converger;
pub(crate) mod local_identity_repair;
pub mod message_builder;
pub(crate) mod message_deduper;

pub(crate) use event_deduper::EventDeduper;
pub(crate) use incoming_message_converger::IncomingMessageConverger;
pub(crate) use local_identity_repair::LocalIdentityRepairService;
pub use message_builder::{
    BuildCardRequest, BuildLinkCardRequest, BuildLocationRequest, BuildMiniProgramRequest,
    BuildRichDocRequest, BuildScheduleRequest, BuildStickerRequest, MessageBuilderService,
};
pub(crate) use message_deduper::MessageDeduper;
