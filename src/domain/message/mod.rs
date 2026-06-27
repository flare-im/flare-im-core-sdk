mod actor;
mod content_policy;
mod delivery;
mod draft;
mod locator;
mod mutation;
mod transport;

pub use actor::{MessageActor, ResolvedMessage};
pub use content_policy::MessageContentPolicy;
pub use delivery::{
    DeliveryLocalSnapshot, InFlightReconcileDecision, IncomingMessageConvergenceDecision,
    MessageDeliveryService, PendingDispatchDecision, REASON_MAX_RETRIES_EXCEEDED,
    REASON_ORPHAN_RECOVERED, REASON_PENDING_ANOTHER_ACCOUNT, REASON_RECONCILED_FAILED,
    REASON_SEND_FAILED_BEFORE_ACK_MAX_RETRIES, REASON_TIMEOUT_AFTER_RETRIES, RetryDecision,
};
pub use draft::MessageDraftService;
pub use locator::MessageLocatorService;
pub(crate) use mutation::{
    DELETE_SCOPE_CONVERSATION_GLOBAL, DELETE_SCOPE_USER_PRIVATE, DELETE_TYPE_SOFT,
};
pub use mutation::{MessageLocalUpdate, MessageMutationPlan, MessageMutationService};
pub use transport::MessageTransportAction;
