pub use crate::generated::contract::{
    API_CONTRACT_VERSION, API_OPERATIONS, BINDING_CONTRACT_VERSION, ERROR_CODES,
    ERROR_CONTRACT_VERSION, EVENT_CONTRACT_VERSION, EVENT_DESCRIPTORS, ErrorCode, EventDescriptor,
    MESSAGE_BUILD_OPS, MessageBuildCatalogEntry,
};

pub use crate::generated::contract::ApiOperation;

pub fn find_api_operation(id: &str) -> Option<&'static ApiOperation> {
    API_OPERATIONS.iter().find(|operation| operation.id == id)
}

pub fn find_event_by_id(id: &str) -> Option<&'static EventDescriptor> {
    EVENT_DESCRIPTORS.iter().find(|event| event.id == id)
}

pub fn find_event_by_code(code: i32) -> Option<&'static EventDescriptor> {
    EVENT_DESCRIPTORS.iter().find(|event| event.c_code == code)
}

pub fn find_error_code(name: &str) -> Option<&'static ErrorCode> {
    ERROR_CODES.iter().find(|error| error.name == name)
}
