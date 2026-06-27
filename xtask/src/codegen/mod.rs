use std::path::PathBuf;

mod bridge;
mod docs;
mod expanded_spec;
mod naming;
mod orchestrator;
mod platform_adapter;
mod platform_api;
mod platform_contract;
mod snapshot;
mod typescript_adapter;
mod typescript_contract;
mod wire_boundary;

#[derive(Clone, Debug)]
pub(crate) struct GeneratedTextTarget {
    pub(crate) path: PathBuf,
    pub(crate) body: String,
}

pub(crate) fn single_trailing_newline(body: &str) -> String {
    format!("{}\n", body.trim_end_matches('\n'))
}

pub(crate) use bridge::emit_bridge_files;
pub(crate) use docs::emit_doc_files;
pub(crate) use expanded_spec::emit_expanded_client_spec_file;
pub(crate) use naming::{
    camel_const, cangjie_identifier, facade_prop, json_quote, kotlin_model_package_imports,
    listener_interface_name, lower_first, model_package_suffix, pascal_case, screaming_snake,
    snake_case, swift_identifier, ts_api_interface_name, ts_api_module_key, ts_model_from_json_fn,
    ts_model_to_map_fn,
};
pub(crate) use orchestrator::run_codegen;
pub(crate) use platform_adapter::emit_platform_adapter_files;
pub(crate) use platform_api::{
    arkts_api_type, cangjie_api_arg, cangjie_api_type, dart_api_type, emit_platform_api_files,
    kotlin_api_module_dir, kotlin_api_type, swift_api_type,
};
pub(crate) use platform_contract::emit_platform_contract_files;
pub(crate) use typescript_adapter::emit_typescript_adapter_files;
pub(crate) use typescript_contract::emit_typescript_contract_files;
pub(crate) use wire_boundary::{emit_wire_boundaries, wire_boundary_targets};
