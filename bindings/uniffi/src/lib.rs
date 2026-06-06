//! UniFFI binding facade over contract-generated metadata.
//!
//! Full UniFFI API surface should be layered on `bindings/runtime` dispatch;
//! extend `bindings/contract/*.json` and run `make -C bindings codegen`.

pub mod generated;

pub use generated::client_config::{client_config_contract_json, client_init_request_example_json};
pub use generated::contract::BINDING_CONTRACT_VERSION;
pub use generated::events::EVENT_CODE_TABLE;
pub use generated::invoke::invoke_contract_api;
pub use generated::types::{BindingErrorCode, BindingEventId};

pub fn binding_contract_version() -> &'static str {
    BINDING_CONTRACT_VERSION
}
