use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    GeneratedTextTarget, cangjie_identifier, child_arr, facade_prop, is_known_ts_model_type,
    kotlin_model_package_imports, listener_payloads, load_expanded_client_spec, pascal_case,
    remove_output_paths, single_trailing_newline, str_field, swift_identifier,
    ts_api_interface_name, ts_api_module_key, upsert_text_file,
};

pub(crate) fn emit_platform_api_files(root: &Path, check: bool) -> Result<()> {
    let spec = load_expanded_client_spec(root)?;
    let mut drifted = Vec::new();
    if !check {
        clean_platform_api_outputs(root)?;
    }
    for target in platform_api_targets(root, &spec)? {
        let body = single_trailing_newline(&target.body);
        upsert_text_file(&target.path, &body, check, &mut drifted)?;
    }
    if !check {
        clean_retired_platform_api_shims(root)?;
    }
    if !drifted.is_empty() {
        let details = drifted.join("\n  - ");
        bail!("Rust-owned platform API output drifted:\n  - {details}");
    }
    if !check {
        println!("Rust-owned platform API artifacts generated");
    }
    Ok(())
}

fn platform_api_targets(root: &Path, spec: &Value) -> Result<Vec<GeneratedTextTarget>> {
    let mut targets = Vec::new();
    targets.extend(emit_dart_api_targets(root, spec));
    targets.extend(emit_kotlin_api_targets(root, spec));
    targets.extend(emit_swift_api_targets(root, spec));
    targets.extend(emit_arkts_api_targets(root, spec));
    targets.extend(emit_cangjie_api_targets(root, spec));
    Ok(targets)
}

fn clean_platform_api_outputs(root: &Path) -> Result<()> {
    remove_output_paths(platform_api_roots(root))?;
    clean_retired_platform_api_shims(root)
}

fn clean_retired_platform_api_shims(root: &Path) -> Result<()> {
    for path in [
        root.join("packages/flare-core-flutter-sdk/lib/src/callbacks.dart"),
        root.join(
            "packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/generated/Callbacks.kt",
        ),
        root.join("packages/flare-core-harmony-arkts-sdk/src/main/ets/generated/callbacks.ets"),
    ] {
        if path.is_file() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

fn platform_api_roots(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("packages/flare-core-flutter-sdk/lib/src/api"),
        root.join("packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/api"),
        root.join("packages/flare-core-apple-sdk/Sources/FlareCoreAppleSDK/Api"),
        root.join("packages/flare-core-harmony-arkts-sdk/src/main/ets/api"),
        root.join("packages/flare-core-harmony-cangjie-sdk/src/api"),
        root.join("packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/generated/api"),
    ]
}

fn method_summary(method: &Value) -> String {
    let extra = if method.get("dispatchOp").is_some() {
        format!(", dispatch op `{}`", str_field(method, "dispatchOp"))
    } else {
        String::new()
    };
    format!(
        "{} maps to `{}` via `{}`{extra}. Operation: `{}`.",
        str_field(method, "name"),
        str_field(method, "cApi"),
        str_field(method, "transport"),
        str_field(method, "operation")
    )
}

pub(crate) fn dart_api_type(name: &str, spec: &Value) -> String {
    match name {
        "Unit" | "DisposeRequest" => "void".to_string(),
        "BooleanResponse" => "bool".to_string(),
        "ConnectionStateResponse" => "ConnectionState".to_string(),
        _ if is_known_ts_model_type(name, spec) => name.to_string(),
        _ => "Map<String, Object?>".to_string(),
    }
}

pub(crate) fn kotlin_api_type(name: &str, spec: &Value) -> String {
    match name {
        "Unit" | "DisposeRequest" => "Unit".to_string(),
        "BooleanResponse" => "Boolean".to_string(),
        "ConnectionStateResponse" => "ConnectionState".to_string(),
        _ if is_known_ts_model_type(name, spec) => name.to_string(),
        _ => "Map<String, Any?>".to_string(),
    }
}

pub(crate) fn swift_api_type(name: &str, spec: &Value) -> String {
    match name {
        "Unit" | "DisposeRequest" => "Void".to_string(),
        "BooleanResponse" => "Bool".to_string(),
        "ConnectionStateResponse" => "ConnectionState".to_string(),
        _ if is_known_ts_model_type(name, spec) => name.to_string(),
        _ => "[String: AnySendable]".to_string(),
    }
}

pub(crate) fn arkts_api_type(name: &str, spec: &Value) -> String {
    match name {
        "Unit" | "DisposeRequest" => "void".to_string(),
        "BooleanResponse" => "boolean".to_string(),
        "ConnectionStateResponse" => "ConnectionState".to_string(),
        _ if is_known_ts_model_type(name, spec) => name.to_string(),
        _ => "Record<string, Object>".to_string(),
    }
}

pub(crate) fn cangjie_api_type(name: &str, spec: &Value) -> String {
    match name {
        "Unit" | "DisposeRequest" => "Unit".to_string(),
        "BooleanResponse" => "BooleanResponse".to_string(),
        "ConnectionStateResponse" => "ConnectionState".to_string(),
        "JsonValue" => "JsonValue".to_string(),
        _ if is_known_ts_model_type(name, spec) => name.to_string(),
        _ => "String".to_string(),
    }
}

fn emit_dart_api_targets(root: &Path, spec: &Value) -> Vec<GeneratedTextTarget> {
    let api_root = root.join("packages/flare-core-flutter-sdk/lib/src/api");
    let modules_root = api_root.join("modules");
    let mut targets = Vec::new();
    for module in child_arr(spec, "modules") {
        let key = ts_api_module_key(module);
        targets.push(GeneratedTextTarget {
            path: modules_root.join(format!("{key}.dart")),
            body: emit_dart_api_module(spec, module),
        });
    }
    let mut module_exports = vec!["// GENERATED. Do not edit by hand.".to_string()];
    for module in child_arr(spec, "modules") {
        module_exports.push(format!(
            "export '{}.dart' show {};",
            ts_api_module_key(module),
            ts_api_interface_name(module)
        ));
    }
    targets.push(GeneratedTextTarget {
        path: modules_root.join("modules.dart"),
        body: module_exports.join("\n"),
    });
    targets.push(GeneratedTextTarget {
        path: api_root.join("connection_state.dart"),
        body: [
            "// GENERATED. Do not edit by hand.",
            "",
            "enum ConnectionState {",
            "  disconnected,",
            "  connecting,",
            "  connected,",
            "  ready,",
            "  reconnecting,",
            "}",
        ]
        .join("\n"),
    });
    let mut client_lines = vec![
        "// GENERATED. Do not edit by hand.".to_string(),
        "import 'modules/modules.dart';".to_string(),
        String::new(),
        "typedef JsonObject = Map<String, Object?>;".to_string(),
        String::new(),
        "/// Root SDK client. Create one instance per app/session boundary.".to_string(),
        "abstract interface class FlareImClient implements SessionApi {".to_string(),
    ];
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        client_lines.push(format!("  /// {}", str_field(module, "description")));
        client_lines.push(format!(
            "  {} get {};",
            ts_api_interface_name(module),
            facade_prop(module)
        ));
    }
    client_lines.push("}".to_string());
    targets.push(GeneratedTextTarget {
        path: api_root.join("client.dart"),
        body: client_lines.join("\n"),
    });
    targets.push(GeneratedTextTarget {
        path: api_root.join("api.dart"),
        body: [
            "// GENERATED. Do not edit by hand.",
            "export 'client.dart';",
            "export 'connection_state.dart';",
            "export 'modules/modules.dart';",
        ]
        .join("\n"),
    });
    targets
}

fn dart_module_needs_models(spec: &Value, module: &Value) -> bool {
    child_arr(module, "methods").iter().any(|method| {
        [str_field(method, "request"), str_field(method, "response")]
            .into_iter()
            .any(|type_name| is_known_ts_model_type(type_name, spec))
    })
}

fn emit_dart_api_module(spec: &Value, module: &Value) -> String {
    let iface = ts_api_interface_name(module);
    let mut lines = vec![
        "// GENERATED. Do not edit by hand.".to_string(),
        format!(
            "// Module API: `{}` — {}",
            str_field(module, "key"),
            str_field(module, "description")
        ),
    ];
    let needs_models = dart_module_needs_models(spec, module);
    if str_field(module, "key") == "connection" {
        lines.push("import '../connection_state.dart';".to_string());
        if needs_models {
            lines.push("import '../../model/model.dart';".to_string());
        }
    } else if needs_models {
        lines.push("import '../../model/model.dart';".to_string());
    }
    if str_field(module, "key") == "events" {
        lines.push("import '../../model/model.dart';".to_string());
        lines.push("import '../../listener/listener.dart';".to_string());
    } else if str_field(module, "key") == "messages" {
        lines.push("import '../../callback/callback.dart';".to_string());
    }
    lines.extend([
        String::new(),
        format!("/// {}", str_field(module, "description")),
        format!("abstract interface class {iface} {{"),
    ]);
    for method in child_arr(module, "methods") {
        let req = dart_api_type(str_field(method, "request"), spec);
        let res = dart_api_type(str_field(method, "response"), spec);
        let arg = if str_field(method, "name") == "sendMessage" {
            "SendMessageRequest request, [MessageSendCallback? callback]".to_string()
        } else if req == "void" {
            String::new()
        } else {
            format!("{req} request")
        };
        lines.push(format!("  /// {}", method_summary(method)));
        lines.push(format!(
            "  Future<{res}> {}({arg});",
            str_field(method, "name")
        ));
    }
    if str_field(module, "key") == "events" {
        lines.extend([
            "  /// Registers a listener object for typed SDK runtime notifications.".to_string(),
            "  EventSubscription addEventListener(FlareImEventListener listener);".to_string(),
            "  /// Removes one local listener registration.".to_string(),
            "  void removeEventListener(EventSubscription subscription);".to_string(),
        ]);
        for listener in child_arr(spec, "listeners") {
            lines.push(format!("  /// {}", str_field(listener, "description")));
            lines.push(format!(
                "  EventSubscription {}(EventCallback<{}> listener);",
                str_field(listener, "name"),
                str_field(listener, "payload")
            ));
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn emit_kotlin_api_targets(root: &Path, spec: &Value) -> Vec<GeneratedTextTarget> {
    let api_root = root.join("packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/api");
    let mut targets = Vec::new();
    for module in child_arr(spec, "modules") {
        targets.push(GeneratedTextTarget {
            path: api_root
                .join(kotlin_api_module_dir(module))
                .join(format!("{}.kt", ts_api_interface_name(module))),
            body: emit_kotlin_api_module(spec, module),
        });
    }
    targets.push(GeneratedTextTarget {
        path: api_root.join("ConnectionState.kt"),
        body: [
            "package com.flare.im.api",
            "",
            "/** GENERATED. Do not edit by hand. */",
            "enum class ConnectionState { DISCONNECTED, CONNECTING, CONNECTED, READY, RECONNECTING }",
        ]
        .join("\n"),
    });
    let mut api_imports = vec!["import com.flare.im.api.session.SessionApi".to_string()];
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        api_imports.push(format!(
            "import com.flare.im.api.{}.{}",
            kotlin_api_module_dir(module),
            ts_api_interface_name(module)
        ));
    }
    let mut client = vec!["package com.flare.im.api".to_string(), String::new()];
    client.extend(api_imports);
    client.extend([
        String::new(),
        "/** GENERATED. Do not edit by hand. */".to_string(),
        "/** Root SDK client. Create one instance per app/session boundary. */".to_string(),
        "interface FlareImClient : SessionApi {".to_string(),
    ]);
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        let prop = facade_prop(module);
        client.push(format!("    /** {} */", str_field(module, "description")));
        client.push(format!("    val {prop}: {}Api", pascal_case(prop)));
    }
    client.push("}".to_string());
    targets.push(GeneratedTextTarget {
        path: api_root.join("FlareImClient.kt"),
        body: client.join("\n"),
    });
    targets
}

pub(crate) fn kotlin_api_module_dir(module: &Value) -> String {
    if str_field(module, "facade") == "client" {
        "session".to_string()
    } else {
        facade_prop(module).to_ascii_lowercase()
    }
}

fn kotlin_api_module_package(module: &Value) -> String {
    format!("com.flare.im.api.{}", kotlin_api_module_dir(module))
}

fn emit_kotlin_api_module(spec: &Value, module: &Value) -> String {
    let iface = ts_api_interface_name(module);
    let mut lines = vec![
        format!("package {}", kotlin_api_module_package(module)),
        String::new(),
        "import com.flare.im.api.ConnectionState".to_string(),
        "import com.flare.im.callback.*".to_string(),
        "import com.flare.im.listener.*".to_string(),
    ];
    lines.extend(kotlin_model_package_imports(spec));
    lines.extend([
        String::new(),
        "/** GENERATED. Do not edit by hand. */".to_string(),
        format!("/** {} */", str_field(module, "description")),
        format!("interface {iface} {{"),
    ]);
    for method in child_arr(module, "methods") {
        let req = kotlin_api_type(str_field(method, "request"), spec);
        let res = kotlin_api_type(str_field(method, "response"), spec);
        let arg = if str_field(method, "name") == "sendMessage" {
            "request: SendMessageRequest, callback: MessageSendCallback? = null".to_string()
        } else if req == "Unit" {
            String::new()
        } else {
            format!("request: {req}")
        };
        lines.push(format!("    /** {} */", method_summary(method)));
        lines.push(format!(
            "    suspend fun {}({arg}): {res}",
            str_field(method, "name")
        ));
    }
    if str_field(module, "key") == "events" {
        lines.extend([
            "    fun addEventListener(listener: FlareImEventListener): EventSubscription"
                .to_string(),
            "    fun removeEventListener(subscription: EventSubscription)".to_string(),
        ]);
        for listener in child_arr(spec, "listeners") {
            lines.push(format!(
                "    fun {}(listener: EventCallback<{}>): EventSubscription",
                str_field(listener, "name"),
                str_field(listener, "payload")
            ));
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn emit_swift_api_targets(root: &Path, spec: &Value) -> Vec<GeneratedTextTarget> {
    let api_root = root.join("packages/flare-core-apple-sdk/Sources/FlareCoreAppleSDK/Api");
    let mut targets = Vec::new();
    for module in child_arr(spec, "modules") {
        let iface = ts_api_interface_name(module);
        targets.push(GeneratedTextTarget {
            path: api_root.join("Modules").join(format!("{iface}.swift")),
            body: emit_swift_api_module(spec, module),
        });
    }
    let mut client = vec![
        "import Foundation".to_string(),
        String::new(),
        "/// GENERATED. Do not edit by hand.".to_string(),
        "public struct AnySendable: @unchecked Sendable { public let value: Any; public init(_ value: Any) { self.value = value } }".to_string(),
        "public enum ConnectionState: String, Sendable { case disconnected, connecting, connected, ready, reconnecting }".to_string(),
        String::new(),
        "public protocol FlareImClientProtocol: SessionApiProtocol {".to_string(),
    ];
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        client.push(format!(
            "    var {}: any {}Protocol {{ get }}",
            facade_prop(module),
            ts_api_interface_name(module)
        ));
    }
    client.push("}".to_string());
    targets.push(GeneratedTextTarget {
        path: api_root.join("FlareImClientApi.swift"),
        body: client.join("\n"),
    });
    targets
}

fn emit_swift_api_module(spec: &Value, module: &Value) -> String {
    let iface = ts_api_interface_name(module);
    let mut lines = vec![
        "import Foundation".to_string(),
        String::new(),
        "/// GENERATED. Do not edit by hand.".to_string(),
        format!("/// {}", str_field(module, "description")),
        format!("public protocol {iface}Protocol: AnyObject {{"),
    ];
    for method in child_arr(module, "methods") {
        let req = swift_api_type(str_field(method, "request"), spec);
        let res = swift_api_type(str_field(method, "response"), spec);
        let arg = if str_field(method, "name") == "sendMessage" {
            "_ request: SendMessageRequest, callback: (any MessageSendCallback)?".to_string()
        } else if req == "Void" {
            String::new()
        } else {
            format!("_ request: {req}")
        };
        lines.push(format!(
            "    func {}({arg}) async throws -> {res}",
            swift_identifier(str_field(method, "name"))
        ));
    }
    if str_field(module, "key") == "events" {
        lines.extend([
            "    func addEventListener(_ listener: any FlareImEventListener) -> any EventSubscription".to_string(),
            "    func removeEventListener(_ subscription: any EventSubscription)".to_string(),
        ]);
        for listener in child_arr(spec, "listeners") {
            lines.push(format!(
                "    func {}(_ listener: @escaping EventCallback<{}>) -> any EventSubscription",
                str_field(listener, "name"),
                str_field(listener, "payload")
            ));
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn emit_arkts_api_targets(root: &Path, spec: &Value) -> Vec<GeneratedTextTarget> {
    let api_root = root.join("packages/flare-core-harmony-arkts-sdk/src/main/ets/api");
    let mut targets = Vec::new();
    for module in child_arr(spec, "modules") {
        targets.push(GeneratedTextTarget {
            path: api_root
                .join("modules")
                .join(format!("{}.ets", ts_api_module_key(module))),
            body: emit_arkts_api_module(spec, module),
        });
    }
    let mut exports = vec!["// GENERATED. Do not edit by hand.".to_string()];
    for module in child_arr(spec, "modules") {
        exports.push(format!(
            "export type {{ {} }} from './modules/{}';",
            ts_api_interface_name(module),
            ts_api_module_key(module)
        ));
    }
    exports.extend([
        "export type { FlareImClient } from './client';".to_string(),
        "export type { ConnectionState } from './client';".to_string(),
    ]);
    targets.push(GeneratedTextTarget {
        path: api_root.join("index.ets"),
        body: exports.join("\n"),
    });
    let mut client = vec![
        "// GENERATED. Do not edit by hand.".to_string(),
        "import { SessionApi } from './modules/session';".to_string(),
    ];
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        client.push(format!(
            "import type {{ {} }} from './modules/{}';",
            ts_api_interface_name(module),
            ts_api_module_key(module)
        ));
    }
    client.extend([
        String::new(),
        "export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'ready' | 'reconnecting';".to_string(),
        String::new(),
        "export interface FlareImClient extends SessionApi {".to_string(),
    ]);
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        client.push(format!(
            "  readonly {}: {};",
            facade_prop(module),
            ts_api_interface_name(module)
        ));
    }
    client.push("}".to_string());
    targets.push(GeneratedTextTarget {
        path: api_root.join("client.ets"),
        body: client.join("\n"),
    });
    targets
}

fn emit_arkts_api_module(spec: &Value, module: &Value) -> String {
    let iface = ts_api_interface_name(module);
    let mut imports = vec![format!(
        "import {{ {} }} from '../../model';",
        listener_payloads(spec).join(", ")
    )];
    if matches!(str_field(module, "key"), "events" | "messages") {
        imports.push("import { EventCallback, EventSubscription, FlareImEventListener, MessageSendCallback } from '../../listener';".to_string());
    }
    let mut lines = vec!["// GENERATED. Do not edit by hand.".to_string()];
    lines.extend(imports);
    lines.extend([
        String::new(),
        format!("/** {} */", str_field(module, "description")),
        format!("export interface {iface} {{"),
    ]);
    for method in child_arr(module, "methods") {
        let req = arkts_api_type(str_field(method, "request"), spec);
        let res = arkts_api_type(str_field(method, "response"), spec);
        let arg = if str_field(method, "name") == "sendMessage" {
            "request: SendMessageRequest, callback?: MessageSendCallback".to_string()
        } else if req == "void" {
            String::new()
        } else {
            format!("request: {req}")
        };
        lines.push(format!(
            "  {}({arg}): Promise<{res}>;",
            str_field(method, "name")
        ));
    }
    if str_field(module, "key") == "events" {
        lines.extend([
            "  addEventListener(listener: FlareImEventListener): EventSubscription;".to_string(),
            "  removeEventListener(subscription: EventSubscription): void;".to_string(),
        ]);
        for listener in child_arr(spec, "listeners") {
            lines.push(format!(
                "  {}(listener: EventCallback<{}>): EventSubscription;",
                str_field(listener, "name"),
                str_field(listener, "payload")
            ));
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

pub(crate) fn cangjie_api_arg(method: &Value, spec: &Value) -> String {
    let req = cangjie_api_type(str_field(method, "request"), spec);
    if req == "Unit" {
        String::new()
    } else if is_known_ts_model_type(str_field(method, "request"), spec) {
        format!("request: {req}")
    } else {
        "requestJson: String".to_string()
    }
}

fn emit_cangjie_api_targets(root: &Path, spec: &Value) -> Vec<GeneratedTextTarget> {
    let api_root = root.join("packages/flare-core-harmony-cangjie-sdk/src/api");
    let mut targets = Vec::new();
    for module in child_arr(spec, "modules") {
        let iface = ts_api_interface_name(module);
        targets.push(GeneratedTextTarget {
            path: api_root.join("modules").join(format!("{iface}.cj")),
            body: emit_cangjie_api_module(spec, module),
        });
    }
    let mut lines = vec![
        "package flare_core_harmony_cangjie_sdk.api".to_string(),
        String::new(),
        "import flare_core_harmony_cangjie_sdk.api.modules.*".to_string(),
        String::new(),
        "// GENERATED. Do not edit by hand.".to_string(),
        "public interface FlareImClient <: SessionApi {".to_string(),
    ];
    for module in child_arr(spec, "modules") {
        if str_field(module, "facade") == "client" {
            continue;
        }
        lines.push(format!(
            "    prop {}: {}",
            facade_prop(module),
            ts_api_interface_name(module)
        ));
    }
    lines.push("}".to_string());
    targets.push(GeneratedTextTarget {
        path: api_root.join("FlareImClient.cj"),
        body: lines.join("\n"),
    });
    targets
}

fn emit_cangjie_api_module(spec: &Value, module: &Value) -> String {
    let iface = ts_api_interface_name(module);
    let mut lines = vec![
        "package flare_core_harmony_cangjie_sdk.api.modules".to_string(),
        String::new(),
        "import flare_core_harmony_cangjie_sdk.model.*".to_string(),
        String::new(),
        "// GENERATED. Do not edit by hand.".to_string(),
        format!("// {}", str_field(module, "description")),
        format!("public interface {iface} {{"),
    ];
    for method in child_arr(module, "methods") {
        lines.push(format!(
            "    func {}({}): {}",
            cangjie_identifier(str_field(method, "name")),
            cangjie_api_arg(method, spec),
            cangjie_api_type(str_field(method, "response"), spec)
        ));
    }
    lines.push("}".to_string());
    lines.join("\n")
}
