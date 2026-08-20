use anyhow::Result;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{arr, emit_errors, fail, include_path, include_paths, load_json, spec_dir, str_field};

pub(crate) fn verify_spec(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let spec = spec_dir(root);
    let manifest = load_json(&spec.join("manifest.json"))?;

    let model_groups = include_paths(&manifest, "models")
        .into_iter()
        .map(|path| load_json(&spec.join(path)))
        .collect::<Result<Vec<_>>>()?;
    let modules = include_paths(&manifest, "modules")
        .into_iter()
        .map(|path| load_json(&spec.join(path)))
        .collect::<Result<Vec<_>>>()?;
    let shared_events = include_path(&manifest, "events")
        .map(|path| load_json(&spec.join(path)))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let listeners = include_path(&manifest, "listeners")
        .map(|path| load_json(&spec.join(path)))
        .transpose()?
        .unwrap_or_else(|| json!({}));

    let mut model_names = BTreeSet::new();
    let mut enum_names = BTreeSet::new();
    for group in &model_groups {
        for model in arr(group.get("models").unwrap_or(&Value::Null)) {
            model_names.insert(str_field(model, "name").to_string());
        }
        for enum_value in arr(group.get("enums").unwrap_or(&Value::Null)) {
            enum_names.insert(str_field(enum_value, "name").to_string());
        }
    }
    let known_model_types = model_names
        .union(&enum_names)
        .cloned()
        .collect::<BTreeSet<_>>();
    let scalar_types = [
        "String",
        "Boolean",
        "Int32",
        "Int64",
        "UInt32",
        "UInt64",
        // Float / Double 是后补的：所有代码生成器（typescript_contract /
        // typescript_adapter / platform_contract / platform_adapter）一直都成对处理
        // 这两个类型，只有这份白名单落在后面——`message_builder` 的经纬度字段用了
        // Double，于是 verify-spec 一直是红的。没人发现是因为**整个 xtask verify
        // 从来没进过 CI**，只在有人手动跑时才执行。
        "Float",
        "Double",
        "JsonObject",
        "StringMap",
        "BinaryMap",
        "StringList",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();
    let builtin_method_types = [
        "Unit",
        "DisposeRequest",
        "BooleanResponse",
        "ConnectionStateResponse",
        "JsonValue",
        "CreateClientRequest",
        "CreateClientResponse",
        "SdkConfig",
        "LoginRequest",
        "MessageDispatchRequest",
        "Subscription",
        "BatchGetUserPresenceResponse",
        "DispatchCapabilityResponse",
        "FfiContractVersion",
        "ListCapabilitiesResponse",
        "ListUserCapabilitiesResponse",
        "MediaAccessUrl",
        "MediaCacheEntry",
        "MediaCacheStats",
        "MediaResolvedAccess",
        "SdkVersion",
        "UserPresence",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<BTreeSet<_>>();

    let mut event_names_by_kind = BTreeMap::<String, BTreeSet<String>>::new();
    for event in arr(shared_events.get("events").unwrap_or(&Value::Null)) {
        let names = arr(event.get("names").unwrap_or(&Value::Null))
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        event_names_by_kind.insert(str_field(event, "type").to_string(), names);
    }

    let mut seen_model_names = BTreeSet::new();
    for group in &model_groups {
        let group_name = str_field(group, "group");
        for enum_value in arr(group.get("enums").unwrap_or(&Value::Null)) {
            let name = str_field(enum_value, "name");
            if !seen_model_names.insert(name.to_string()) {
                fail(&mut errors, format!("duplicate model/enum name: {name}"));
            }
            let values = arr(enum_value.get("values").unwrap_or(&Value::Null))
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            let unique = values.iter().copied().collect::<BTreeSet<_>>();
            if values.len() != unique.len() {
                fail(&mut errors, format!("duplicate enum value in {name}"));
            }
        }
        for model in arr(group.get("models").unwrap_or(&Value::Null)) {
            let name = str_field(model, "name");
            if !seen_model_names.insert(name.to_string()) {
                fail(&mut errors, format!("duplicate model/enum name: {name}"));
            }
            let mut seen_fields = BTreeSet::new();
            let mut seen_wire_fields = BTreeSet::new();
            for field in arr(model.get("fields").unwrap_or(&Value::Null)) {
                let field_name = str_field(field, "name");
                let wire_name = str_field(field, "wireName");
                let field_type = str_field(field, "type");
                let inner_type = field_type.strip_suffix("List").unwrap_or(field_type);
                if !seen_fields.insert(field_name.to_string()) {
                    fail(&mut errors, format!("duplicate field {name}.{field_name}"));
                }
                if !seen_wire_fields.insert(wire_name.to_string()) {
                    fail(
                        &mut errors,
                        format!("duplicate wire field {name}.{wire_name}"),
                    );
                }
                if wire_name != field_name {
                    fail(
                        &mut errors,
                        format!(
                            "wireName must match field name for camelCase SDK JSON: {name}.{field_name} -> {wire_name}"
                        ),
                    );
                }
                if wire_name.contains('_') {
                    fail(
                        &mut errors,
                        format!(
                            "wireName must be camelCase SDK JSON, not snake_case: {name}.{wire_name}"
                        ),
                    );
                }
                if !scalar_types.contains(inner_type) && !known_model_types.contains(inner_type) {
                    fail(
                        &mut errors,
                        format!(
                            "unknown field type {group_name}.{name}.{field_name}: {field_type}"
                        ),
                    );
                }
            }
        }
    }

    let mut seen_operations = BTreeSet::new();
    let mut seen_methods = BTreeSet::new();
    for module in &modules {
        let module_key = str_field(module, "key");
        for method in arr(module.get("methods").unwrap_or(&Value::Null)) {
            let method_name = str_field(method, "name");
            let operation = str_field(method, "operation");
            if !seen_methods.insert(format!("{module_key}.{method_name}")) {
                fail(
                    &mut errors,
                    format!("duplicate method {module_key}.{method_name}"),
                );
            }
            if !seen_operations.insert(operation.to_string()) {
                fail(&mut errors, format!("duplicate operation {operation}"));
            }
            for attr in ["request", "response"] {
                let type_name = str_field(method, attr);
                if !known_model_types.contains(type_name)
                    && !builtin_method_types.contains(type_name)
                    && !type_name.ends_with("Request")
                    && !type_name.ends_with("Response")
                {
                    fail(
                        &mut errors,
                        format!("unknown {attr} type for {operation}: {type_name}"),
                    );
                }
            }
        }
    }

    let known_model_types_ref = known_model_types
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut seen_listeners = BTreeSet::new();
    for listener in arr(listeners.get("listeners").unwrap_or(&Value::Null)) {
        let name = str_field(listener, "name");
        let kind = str_field(listener, "kind");
        let event_name = str_field(listener, "eventName");
        let payload = str_field(listener, "payload");
        if !seen_listeners.insert(name.to_string()) {
            fail(&mut errors, format!("duplicate listener {name}"));
        }
        match event_names_by_kind.get(kind) {
            None => fail(
                &mut errors,
                format!("listener {name} references unknown event kind {kind}"),
            ),
            Some(names) if !names.contains(event_name) => fail(
                &mut errors,
                format!("listener {name} references unknown event {kind}.{event_name}"),
            ),
            _ => {}
        }
        if !known_model_types_ref.contains(payload) {
            fail(
                &mut errors,
                format!("listener {name} references unknown payload {payload}"),
            );
        }
    }

    emit_errors("spec validation error", errors)?;
    println!("sdk spec verified");
    Ok(())
}
