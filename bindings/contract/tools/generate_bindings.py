#!/usr/bin/env python3
"""Generate all binding artifacts from bindings/contract/*.json."""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
CONTRACT_DIR = ROOT / "contract"
RUNTIME_CONTRACT = ROOT / "shared" / "src" / "generated" / "contract.rs"
PLATFORM_OUTS = {
    "c": ROOT / "c" / "src" / "generated" / "contract.rs",
    "wasm": ROOT / "wasm" / "src" / "generated" / "contract.rs",
    "tauri": ROOT / "tauri" / "src" / "generated" / "contract.rs",
    "uniffi": ROOT / "uniffi" / "src" / "generated" / "contract.rs",
}
C_EVENTS_OUT = ROOT / "c" / "src" / "generated" / "events.rs"
C_ERRORS_OUT = ROOT / "c" / "src" / "generated" / "errors.rs"
UNIFFI_TYPES_OUT = ROOT / "uniffi" / "src" / "generated" / "types.rs"


def rust_str(value: Any) -> str:
    if value is None:
        return "None"
    return f"Some({rust_lit(value)})"


def rust_lit(value: Any) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def read_json(name: str) -> Any:
    with (CONTRACT_DIR / name).open("r", encoding="utf-8") as file:
        return json.load(file)


def c_api_parts(value: Any) -> tuple[str | None, str | None]:
    if isinstance(value, list):
        value = value[0] if value else None
    if not isinstance(value, str):
        return None, None
    if ":" in value:
        symbol, dispatch = value.split(":", 1)
        return symbol, dispatch
    return value, None


def method_name_for_build_op(op: str) -> str:
    suffix = op.removeprefix("create_")
    return "build" + "".join(part[:1].upper() + part[1:] for part in suffix.split("_") if part)


def build_op_from_api_id(api_id: str) -> str | None:
    if not api_id.startswith("message_builder.create_"):
        return None
    return api_id.split(".", 1)[1]


def collect_api_operations(apis: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for module in apis.get("modules", []):
        module_id = module.get("id", "")
        for method in module.get("methods", []):
            c_symbol, dispatch_op = c_api_parts(method.get("c"))
            rows.append(
                {
                    "id": method.get("id", ""),
                    "module": module_id,
                    "core": method.get("core"),
                    "c_symbol": c_symbol,
                    "c_dispatch_op": dispatch_op,
                    "tauri": method.get("tauri"),
                    "dev_only": bool(method.get("dev_only", False)),
                }
            )
    return rows


def collect_message_build_ops(api_operations: list[dict[str, Any]]) -> list[dict[str, str]]:
    seen: set[str] = set()
    rows: list[dict[str, str]] = []
    for operation in api_operations:
        op = build_op_from_api_id(operation["id"])
        if not op or op in seen:
            continue
        seen.add(op)
        rows.append(
            {
                "op": op,
                "method": method_name_for_build_op(op),
                "stability": "stable",
                "source_operation": operation["id"],
            }
        )
    return sorted(rows, key=lambda item: item["op"])


def collect_events(events: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for event in events.get("events", []):
        rows.append(
            {
                "id": event.get("id", ""),
                "c_code": int(event.get("cCode", 0)),
                "c_code_name": event.get("cCodeName", ""),
                "tauri": event.get("tauri"),
            }
        )
    return rows


def collect_errors(errors: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        {
            "name": item.get("name", ""),
            "code": int(item.get("code", 0)),
            "meaning": item.get("meaning", ""),
            "core_codes": item.get("coreCodes", []),
        }
        for item in errors.get("cAbi", {}).get("codes", [])
    ]


def c_api_entries(value: Any) -> list[tuple[str, str | None]]:
    values = value if isinstance(value, list) else [value]
    entries: list[tuple[str, str | None]] = []
    for item in values:
        if not isinstance(item, str):
            continue
        symbol, dispatch = c_api_parts(item)
        if symbol:
            entries.append((symbol, dispatch))
    return entries


def collect_c_dispatch_entries(apis: dict[str, Any]) -> list[tuple[str, str, str]]:
    rows: list[tuple[str, str, str]] = []
    for module in apis.get("modules", []):
        for method in module.get("methods", []):
            api_id = method.get("id", "")
            for symbol, dispatch in c_api_entries(method.get("c")):
                if dispatch:
                    rows.append((api_id, symbol, dispatch))
    return rows


def load_contracts() -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]:
    return (
        read_json("manifest.json"),
        read_json("apis.json"),
        read_json("events.json"),
        read_json("errors.json"),
    )


def require_object(name: str, value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{name} must be a JSON object")
    return value


def require_list(name: str, value: Any) -> list[Any]:
    if not isinstance(value, list):
        raise ValueError(f"{name} must be a JSON array")
    return value


def ensure_unique(label: str, values: list[str]) -> None:
    seen: set[str] = set()
    duplicates: list[str] = []
    for value in values:
        if value in seen and value not in duplicates:
            duplicates.append(value)
        seen.add(value)
    if duplicates:
        joined = ", ".join(sorted(duplicates))
        raise ValueError(f"{label} contains duplicate values: {joined}")


def ensure_no_removed_api_aliases(api_ids: list[str]) -> None:
    removed_prefixes = (
        "events.",
        "messages.",
        "conversations.",
        "capabilities.",
    )
    removed_ids = {
        "media.get_file_url",
        "sync.set_conversation_input_state",
    }
    offenders = [
        api_id
        for api_id in api_ids
        if api_id in removed_ids or api_id.startswith(removed_prefixes)
    ]
    if offenders:
        joined = ", ".join(sorted(offenders))
        raise ValueError(
            "apis.json contains removed compatibility API ids; use singular "
            f"canonical contract ids instead: {joined}"
        )


def c_symbol_runtime_group(symbol: str) -> str:
    channel = symbol.removeprefix("flare_").removesuffix("_json")
    if channel == "message_build":
        return "message_build"
    return channel.replace("_dispatch", "")


def dispatch_operation_names(operation: dict[str, Any]) -> list[str]:
    return [operation["op"], *operation.get("aliases", [])]


def validate_contracts() -> None:
    manifest, apis, events, errors = load_contracts()
    dispatch = require_object("dispatch.json", read_json("dispatch.json"))
    direct_invoke = require_object("direct_invoke.json", read_json("direct_invoke.json"))

    require_list("apis.json modules", apis.get("modules"))
    require_list("events.json events", events.get("events"))
    require_object("errors.json cAbi", errors.get("cAbi"))
    require_list("dispatch.json groups", dispatch.get("groups"))
    require_list("direct_invoke.json routes", direct_invoke.get("routes"))

    if not manifest.get("contractVersion"):
        raise ValueError("manifest.json must define contractVersion")
    if not apis.get("apiContractVersion"):
        raise ValueError("apis.json must define apiContractVersion")
    if not events.get("eventContractVersion"):
        raise ValueError("events.json must define eventContractVersion")
    if not errors.get("errorContractVersion"):
        raise ValueError("errors.json must define errorContractVersion")

    api_ids: list[str] = []
    for module in apis["modules"]:
        methods = require_list(f"apis.json module {module.get('id')} methods", module.get("methods"))
        api_ids.extend(method.get("id", "") for method in methods)
    ensure_unique("apis.json method ids", api_ids)
    ensure_no_removed_api_aliases(api_ids)

    event_ids = [event.get("id", "") for event in events["events"]]
    event_codes = [str(event.get("cCode", "")) for event in events["events"]]
    ensure_unique("events.json event ids", event_ids)
    ensure_unique("events.json C event codes", event_codes)

    error_codes = require_list("errors.json cAbi codes", errors["cAbi"].get("codes"))
    ensure_unique("errors.json error names", [item.get("name", "") for item in error_codes])
    ensure_unique("errors.json error codes", [str(item.get("code", "")) for item in error_codes])

    dispatch_groups: dict[str, set[str]] = {}
    for group in dispatch["groups"]:
        group_id = group.get("id", "")
        operations = require_list(f"dispatch.json group {group_id} operations", group.get("operations"))
        aliased = [operation.get("op", "") for operation in operations if operation.get("aliases")]
        if aliased:
            joined = ", ".join(sorted(aliased))
            raise ValueError(
                f"dispatch.json group {group_id} contains removed compatibility aliases: {joined}"
            )
        names = [name for operation in operations for name in dispatch_operation_names(operation)]
        ensure_unique(f"dispatch.json group {group_id} operation names", names)
        dispatch_groups[group_id] = set(names)
    ensure_unique("dispatch.json group ids", list(dispatch_groups))

    routes = [route.get("route", "") for route in direct_invoke["routes"]]
    ensure_unique("direct_invoke.json routes", routes)

    for module in apis["modules"]:
        for method in module.get("methods", []):
            for symbol, dispatch_op in c_api_entries(method.get("c")):
                if not dispatch_op:
                    continue
                group = c_symbol_runtime_group(symbol)
                if group not in dispatch_groups:
                    raise ValueError(
                        f"apis.json method {method.get('id')} references unknown C dispatch group "
                        f"{group!r} via {symbol!r}"
                    )
                if dispatch_op not in dispatch_groups[group]:
                    raise ValueError(
                        f"apis.json method {method.get('id')} references missing dispatch op "
                        f"{group}.{dispatch_op}"
                    )


def render_contract_module() -> str:
    manifest, apis, events, errors = load_contracts()
    api_operations = collect_api_operations(apis)
    build_ops = collect_message_build_ops(api_operations)
    event_rows = collect_events(events)
    error_rows = collect_errors(errors)

    lines: list[str] = [
        "// @generated by bindings/contract/tools/generate_bindings.py",
        "// Do not edit by hand. Edit bindings/contract/*.json and run `make -C bindings codegen`.",
        "#![allow(dead_code)]",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct ApiOperation {",
        "    pub id: &'static str,",
        "    pub module: &'static str,",
        "    pub core: Option<&'static str>,",
        "    pub c_symbol: Option<&'static str>,",
        "    pub c_dispatch_op: Option<&'static str>,",
        "    pub tauri: Option<&'static str>,",
        "    pub dev_only: bool,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct MessageBuildCatalogEntry {",
        "    pub op: &'static str,",
        "    pub method: &'static str,",
        "    pub stability: &'static str,",
        "    pub source_operation: &'static str,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct EventDescriptor {",
        "    pub id: &'static str,",
        "    pub c_code: i32,",
        "    pub c_code_name: &'static str,",
        "    pub tauri: Option<&'static str>,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub struct ErrorCode {",
        "    pub name: &'static str,",
        "    pub code: i32,",
        "    pub meaning: &'static str,",
        "}",
        "",
        f"pub const BINDING_CONTRACT_VERSION: &str = {rust_lit(manifest.get('contractVersion', ''))};",
        f"pub const API_CONTRACT_VERSION: &str = {rust_lit(apis.get('apiContractVersion', ''))};",
        f"pub const EVENT_CONTRACT_VERSION: &str = {rust_lit(events.get('eventContractVersion', ''))};",
        f"pub const ERROR_CONTRACT_VERSION: &str = {rust_lit(errors.get('errorContractVersion', ''))};",
        "",
        "pub const API_OPERATIONS: &[ApiOperation] = &[",
    ]
    for row in api_operations:
        lines.append(
            "    ApiOperation { "
            f"id: {rust_lit(row['id'])}, "
            f"module: {rust_lit(row['module'])}, "
            f"core: {rust_str(row['core'])}, "
            f"c_symbol: {rust_str(row['c_symbol'])}, "
            f"c_dispatch_op: {rust_str(row['c_dispatch_op'])}, "
            f"tauri: {rust_str(row['tauri'])}, "
            f"dev_only: {str(row['dev_only']).lower()} "
            "},"
        )
    lines.extend(["];", "", "pub const MESSAGE_BUILD_OPS: &[MessageBuildCatalogEntry] = &["])
    for row in build_ops:
        lines.append(
            "    MessageBuildCatalogEntry { "
            f"op: {rust_lit(row['op'])}, "
            f"method: {rust_lit(row['method'])}, "
            f"stability: {rust_lit(row['stability'])}, "
            f"source_operation: {rust_lit(row['source_operation'])} "
            "},"
        )
    lines.extend(["];", "", "pub const EVENT_DESCRIPTORS: &[EventDescriptor] = &["])
    for row in event_rows:
        lines.append(
            "    EventDescriptor { "
            f"id: {rust_lit(row['id'])}, "
            f"c_code: {row['c_code']}, "
            f"c_code_name: {rust_lit(row['c_code_name'])}, "
            f"tauri: {rust_str(row['tauri'])} "
            "},"
        )
    lines.extend(["];", "", "pub const ERROR_CODES: &[ErrorCode] = &["])
    for row in error_rows:
        lines.append(
            "    ErrorCode { "
            f"name: {rust_lit(row['name'])}, "
            f"code: {row['code']}, "
            f"meaning: {rust_lit(row['meaning'])} "
            "},"
        )
    lines.extend(["];", ""])
    return rustfmt("\n".join(lines))


def render_platform(platform: str) -> str:
    manifest, apis, events, errors = load_contracts()
    api_operations = collect_api_operations(apis)
    event_rows = collect_events(events)
    error_rows = collect_errors(errors)

    lines = [
        "// @generated by bindings/contract/tools/generate_bindings.py",
        "// Do not edit by hand. Edit bindings/contract/*.json and run `make -C bindings codegen`.",
        "#![allow(dead_code)]",
        "",
        f"pub const PLATFORM_BINDING: &str = {rust_lit(platform)};",
        f"pub const BINDING_CONTRACT_VERSION: &str = {rust_lit(manifest.get('contractVersion', ''))};",
        "",
    ]

    if platform == "c":
        c_symbols: list[str] = []
        c_dispatch_ops: list[tuple[str, str]] = []
        for module in apis.get("modules", []):
            for method in module.get("methods", []):
                if method.get("dev_only", False):
                    continue
                for symbol, dispatch in c_api_entries(method.get("c")):
                    if symbol not in c_symbols:
                        c_symbols.append(symbol)
                    if dispatch:
                        c_dispatch_ops.append((method.get("id", ""), dispatch))
        lines.extend(
            [
                "pub const C_API_SYMBOLS: &[&str] = &[",
                *[f"    {rust_lit(symbol)}," for symbol in sorted(c_symbols)],
                "];",
                "",
                "pub const C_DISPATCH_OPS: &[(&str, &str)] = &[",
                *[
                    f"    ({rust_lit(api_id)}, {rust_lit(dispatch)}),"
                    for api_id, dispatch in sorted(c_dispatch_ops)
                ],
                "];",
            ]
        )

    if platform == "tauri":
        commands = sorted(
            {
                op["tauri"]
                for op in api_operations
                if not op["dev_only"]
                and isinstance(op["tauri"], str)
                and op["tauri"].startswith("sdk_")
            }
        )
        lines.extend(
            [
                "pub const TAURI_COMMANDS: &[&str] = &[",
                *[f"    {rust_lit(command)}," for command in commands],
                "];",
            ]
        )

    if platform == "wasm":
        lines.extend(
            [
                "pub const WASM_ACCEPTS_OPERATION_INVOKE: bool = true;",
                "pub const WASM_CANONICAL_OPERATIONS: &[&str] = &[",
                *[f"    {rust_lit(op['id'])}," for op in api_operations if not op["dev_only"]],
                "];",
            ]
        )

    if platform == "uniffi":
        lines.extend(
            [
                "pub const UNIFFI_CANONICAL_OPERATIONS: &[&str] = &[",
                *[f"    {rust_lit(op['id'])}," for op in api_operations if not op["dev_only"]],
                "];",
            ]
        )

    lines.extend(
        [
            "",
            "pub const EVENT_CODES: &[(&str, i32)] = &[",
            *[f"    ({rust_lit(row['id'])}, {row['c_code']})," for row in event_rows],
            "];",
            "",
            "pub const ERROR_CODES: &[(&str, i32)] = &[",
            *[f"    ({rust_lit(row['name'])}, {row['code']})," for row in error_rows],
            "];",
            "",
        ]
    )
    return rustfmt("\n".join(lines))


def render_c_events() -> str:
    _, _, events, _ = load_contracts()
    event_rows = collect_events(events)
    lines = [
        "// @generated by bindings/contract/tools/generate_bindings.py",
        "// Do not edit by hand. Edit bindings/contract/events.json and run `make -C bindings codegen`.",
        "#![allow(dead_code)]",
        "",
        "pub const FLARE_EVENT_UNKNOWN: i32 = 0;",
    ]
    for row in event_rows:
        lines.append(f"pub const {row['c_code_name']}: i32 = {row['c_code']};")
    lines.extend(
        [
            "",
            "pub fn event_code_by_id(id: &str) -> i32 {",
            "    use crate::generated::contract::EVENT_CODES;",
            "    EVENT_CODES",
            "        .iter()",
            "        .find_map(|(event_id, code)| (*event_id == id).then_some(*code))",
            "        .unwrap_or(FLARE_EVENT_UNKNOWN)",
            "}",
            "",
        ]
    )
    return rustfmt("\n".join(lines))


def render_c_errors() -> str:
    _, _, _, errors = load_contracts()
    error_rows = collect_errors(errors)
    lines = [
        "// @generated by bindings/contract/tools/generate_bindings.py",
        "// Do not edit by hand. Edit bindings/contract/errors.json and run `make -C bindings codegen`.",
        "#![allow(dead_code)]",
        "",
    ]
    for row in error_rows:
        lines.append(f"pub const {row['name']}: i32 = {row['code']};")
    lines.extend(
        [
            "",
            "pub fn error_code_to_c(code: flare_im_core_sdk::ErrorCode) -> i32 {",
            "    use flare_im_core_sdk::ErrorCode;",
            "    match code {",
        ]
    )
    mapping: dict[str, list[str]] = {}
    for row in error_rows:
        for core in row.get("core_codes", []):
            mapping.setdefault(core, []).append(row["name"])
    for core, names in sorted(mapping.items()):
        target = names[0]
        lines.append(f"        ErrorCode::{core} => {target},")
    lines.extend(
        [
            "        _ => FLARE_ERR_INTERNAL,",
            "    }",
            "}",
            "",
        ]
    )
    return rustfmt("\n".join(lines))


def uniffi_error_variant(name: str) -> str:
    stem = name.removeprefix("FLARE_").removeprefix("ERR_")
    if stem == "OK":
        return "Ok"
    return "".join(part[:1].upper() + part[1:].lower() for part in stem.split("_") if part)


def uniffi_event_variant(event_id: str) -> str:
    return "".join(part.capitalize() for part in event_id.replace(".", "_").split("_") if part)


def render_uniffi_types() -> str:
    _, _, events, errors = load_contracts()
    event_rows = collect_events(events)
    error_rows = collect_errors(errors)
    lines = [
        "// @generated by bindings/contract/tools/generate_bindings.py",
        "// Do not edit by hand. Edit bindings/contract/*.json and run `make -C bindings codegen`.",
        "#![allow(dead_code)]",
        "",
        "/// Stable C ABI error codes mirrored for UniFFI consumers.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub enum BindingErrorCode {",
    ]
    for row in error_rows:
        lines.append(f"    {uniffi_error_variant(row['name'])} = {row['code']},")
    lines.extend(
        [
            "}",
            "",
            "/// Canonical SDK event ids for UniFFI consumers.",
            "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
            "pub enum BindingEventId {",
        ]
    )
    for row in event_rows:
        lines.append(f"    {uniffi_event_variant(row['id'])},")
    lines.extend(["}", ""])
    return rustfmt("\n".join(lines))


def rustfmt(content: str) -> str:
    with tempfile.NamedTemporaryFile("w+", encoding="utf-8", suffix=".rs") as file:
        file.write(content)
        file.flush()
        subprocess.run(
            ["rustfmt", "--edition", "2024", "--config", "skip_children=true", file.name],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        file.seek(0)
        return file.read()


def rustfmt_outputs(outputs: dict[Path, str]) -> dict[Path, str]:
    formatted: dict[Path, str] = {}
    for path, content in outputs.items():
        if path.suffix != ".rs":
            formatted[path] = content
            continue
        try:
            formatted[path] = rustfmt(content)
        except subprocess.CalledProcessError as error:
            raise RuntimeError(
                f"rustfmt failed for generated output {path}: {error.stderr}"
            ) from error
    return formatted


def generated_mod(modules: list[str]) -> str:
    return "\n".join(f"pub mod {name};" for name in modules) + "\n"


def generate_event_outputs() -> dict[Path, str]:
    import sys

    tools_dir = str(CONTRACT_DIR / "tools")
    if tools_dir not in sys.path:
        sys.path.insert(0, tools_dir)
    from event_codegen import generate_all

    return generate_all()


def generate_direct_invoke_outputs() -> dict[Path, str]:
    import sys

    tools_dir = str(CONTRACT_DIR / "tools")
    if tools_dir not in sys.path:
        sys.path.insert(0, tools_dir)
    from direct_invoke_codegen import generate_all

    return generate_all()


def generate_dispatch_outputs() -> dict[Path, str]:
    import sys

    tools_dir = str(CONTRACT_DIR / "tools")
    if tools_dir not in sys.path:
        sys.path.insert(0, tools_dir)
    from dispatch_codegen import generate_all

    return generate_all()


def generate_client_config_outputs() -> dict[Path, str]:
    import sys

    tools_dir = str(CONTRACT_DIR / "tools")
    if tools_dir not in sys.path:
        sys.path.insert(0, tools_dir)
    from client_config_codegen import generate_all

    return generate_all()


def generate_c_typed_abi_outputs() -> dict[Path, str]:
    import sys

    tools_dir = str(CONTRACT_DIR / "tools")
    if tools_dir not in sys.path:
        sys.path.insert(0, tools_dir)
    from c_abi_codegen import generate_all

    return generate_all()


def generate_platform_outputs() -> dict[Path, str]:
    import sys

    tools_dir = str(CONTRACT_DIR / "tools")
    if tools_dir not in sys.path:
        sys.path.insert(0, tools_dir)
    from platform_codegen import generate_all

    return generate_all()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    validate_contracts()

    outputs: dict[Path, str] = {
        RUNTIME_CONTRACT: render_contract_module(),
        C_EVENTS_OUT: render_c_events(),
        C_ERRORS_OUT: render_c_errors(),
        UNIFFI_TYPES_OUT: render_uniffi_types(),
    }
    outputs.update({path: render_platform(platform) for platform, path in PLATFORM_OUTS.items()})
    outputs.update(generate_dispatch_outputs())
    outputs.update(generate_direct_invoke_outputs())
    outputs.update(generate_event_outputs())
    outputs.update(generate_platform_outputs())
    outputs.update(generate_c_typed_abi_outputs())
    outputs.update(generate_client_config_outputs())
    outputs[RUNTIME_CONTRACT.parent / "mod.rs"] = (
        "pub mod client_config;\npub mod contract;\npub mod direct_invoke;\npub mod dispatch;\n"
        "pub mod event_codes;\npub mod event_registry;\n\n"
        "pub use client_config::{\n"
        "    CLIENT_CONFIG_CONTRACT_JSON, CLIENT_INIT_REQUEST_EXAMPLE_JSON,\n"
        "};\n"
        "pub use dispatch::{\n"
        "    CAPABILITY_DISPATCH_OPERATIONS, CONVERSATION_DISPATCH_OPERATIONS,\n"
        "    MEDIA_DISPATCH_OPERATIONS, MESSAGE_BUILD_OPERATIONS, MESSAGE_DISPATCH_OPERATIONS,\n"
        "};\n"
    )
    outputs[ROOT / "c" / "src" / "generated" / "mod.rs"] = generated_mod(
        ["client_config", "contract", "events", "errors", "json_dispatch", "invoke", "typed_abi"]
    )
    outputs[ROOT / "uniffi" / "src" / "generated" / "mod.rs"] = generated_mod(
        ["client_config", "contract", "types", "invoke", "events"]
    )
    outputs[ROOT / "wasm" / "src" / "generated" / "mod.rs"] = generated_mod(
        ["client_config", "contract", "bindings", "events"]
    )
    outputs[ROOT / "tauri" / "src" / "generated" / "mod.rs"] = (
        "pub mod contract;\npub mod event_emit;\npub mod handler;\npub mod invoke;\n"
    )
    outputs = rustfmt_outputs(outputs)

    if args.check:
        stale = []
        for path, content in outputs.items():
            current = path.read_text(encoding="utf-8") if path.exists() else ""
            if current != content:
                stale.append(path)
        if stale:
            for path in stale:
                print(f"{path} is stale; run `make -C bindings codegen`")
            return 1
        return 0

    for path, content in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        if path.parent.name != "dispatch" or path.name == "mod.rs":
            print(f"generated {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
