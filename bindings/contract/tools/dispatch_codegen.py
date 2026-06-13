#!/usr/bin/env python3
"""Generate JSON dispatch match arms from contract/dispatch.json."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

BINDINGS_ROOT = Path(__file__).resolve().parents[2]
CONTRACT_DIR = BINDINGS_ROOT / "contract"
DISPATCH_JSON = CONTRACT_DIR / "dispatch.json"
DISPATCH_OUT_DIR = BINDINGS_ROOT / "shared" / "src" / "generated" / "dispatch"


def owned_clone_expr(value_expr: str) -> str:
    if value_expr.startswith("&"):
        return f"{value_expr[1:]}.clone()"
    return f"{value_expr}.clone()"


def wire_key(name: str) -> str:
    if name.startswith("@"):
        return name
    parts = name.split("_")
    return parts[0] + "".join(part[:1].upper() + part[1:] for part in parts[1:])


def load_dispatch() -> dict[str, Any]:
    if not DISPATCH_JSON.exists():
        raise FileNotFoundError(
            f"{DISPATCH_JSON} is missing; maintain dispatch ops in contract/dispatch.json"
        )
    with DISPATCH_JSON.open("r", encoding="utf-8") as file:
        return json.load(file)


def parse_arg(spec: list[Any]) -> dict[str, Any]:
    if spec[0] == "literal":
        if len(spec) == 2:
            return {"name": "literal", "kind": spec[1]}
        return {"name": "literal", "kind": spec[1], "value": spec[2]}
    if len(spec) == 2:
        name, kind = spec
        out: dict[str, Any] = {"name": name, "kind": kind}
        if ":" in kind:
            out["name"], out["kind"] = name, kind
        return out
    name, kind, extra = spec
    out = {"name": name, "kind": kind}
    if isinstance(extra, dict):
        out.update(extra)
    elif isinstance(extra, int):
        out["default"] = extra
    elif isinstance(extra, bool):
        out["value"] = extra
    elif isinstance(extra, str):
        out["value"] = extra
    return out


def op_patterns(op: dict[str, Any]) -> str:
    parts = [f'"{op["op"]}"']
    for alias in op.get("aliases", []):
        parts.append(f'"{alias}"')
    return " | ".join(parts)


def operation_names(op: dict[str, Any]) -> list[str]:
    return [op["op"], *op.get("aliases", [])]


def render_arg_extract(arg: dict[str, Any], group: dict[str, Any], value_expr: str) -> tuple[str, str]:
    """Return (let_bindings, rust_expr) for one method argument."""
    name = arg["name"]
    kind = arg["kind"]
    g = value_expr
    key = arg.get("wire", wire_key(name))

    if name == "@value" and kind.startswith("deserialize:"):
        ty = kind.split(":", 1)[1]
        v = "bound_value"
        return (
            f"let {v}: {ty} = from_value({owned_clone_expr(g)}, {json.dumps(ty)})?;",
            v,
        )

    if kind == "str_ref":
        v = f'{name}_s'
        return (f'let {v} = json_string({g}, "{key}")?;', f"&{v}")
    if kind == "conversation_id":
        v = f"{name}_s"
        return (f"let {v} = conversation_id({g})?;", f"&{v}")
    if kind == "optional_str_ref":
        v = f"{name}_o"
        return (f'let {v} = optional_string({g}, "{key}");', f"{v}.as_deref()")
    if kind == "optional_string":
        v = f"{name}_o"
        return (f'let {v} = optional_string({g}, "{key}");', f"{v}")
    if kind == "string":
        v = f"{name}_s"
        return (f'let {v} = json_string({g}, "{key}")?;', f"{v}")
    if kind == "u64":
        v = f"{name}_v"
        return (f'let {v} = json_u64({g}, "{key}")?;', f"{v}")
    if kind == "i64":
        v = f"{name}_v"
        return (f'let {v} = json_i64({g}, "{key}")?;', f"{v}")
    if kind == "bool":
        v = f"{name}_v"
        return (f'let {v} = json_bool({g}, "{key}")?;', f"{v}")
    if kind == "bool_default_false":
        v = f"{name}_v"
        return (f'let {v} = json_bool({g}, "{key}").unwrap_or(false);', f"{v}")
    if kind == "mark_type":
        v = f"{name}_v"
        return (f'let {v} = parse_mark_type(json_i32({g}, "{key}")?);', f"{v}")
    if kind == "conversation_type":
        v = f"{name}_v"
        return (f"let {v} = conversation_type({g})?;", f"&{v}")
    if kind == "vec_string":
        v = f"{name}_v"
        pass_as = arg.get("pass", "owned")
        expr = f"&{v}" if pass_as == "ref" else f"{v}"
        return (f'let {v} = json_vec_string({g}, "{key}")?;', expr)
    if kind == "vec_im_message":
        v = f"{name}_v"
        return (f'let {v} = json_vec_message({g}, "{key}")?;', f"{v}")
    if kind == "optional_vec_string":
        v = f"{name}_v"
        return (
            f'let {v} = optional_value::<Vec<String>>({g}, "{key}")?;',
            f"{v}",
        )
    if kind == "optional_hashmap_string":
        v = f"{name}_v"
        return (
            f'let {v} = optional_value::<std::collections::HashMap<String, String>>({g}, "{key}")?;',
            f"{v}",
        )
    if kind == "optional_built_content":
        v = f"{name}_v"
        return (
            f"""let {v} = match {g}.get("quotedContent") {{
            Some(v) => Some(built_content_from_value(v)?),
            None => None,
        }};""",
            f"{v}",
        )
    if kind == "optional_u32":
        v = f"{name}_v"
        return (f'let {v} = optional_u32({g}, "{key}");', f"{v}")
    if kind == "im_message":
        return ("", f"message_from_params({g})?")
    if kind.startswith("deserialize:"):
        ty = kind.split(":", 1)[1]
        v = f"{name}_v"
        label = ty.replace(":", " ")
        return (
            f"let {v}: {ty} = from_value({owned_clone_expr(g)}, {json.dumps(label)})?;",
            f"{v}",
        )
    if kind == "rich_doc_edit_request":
        return (
            "",
            f"from_value::<EditRichDocJson>({owned_clone_expr(g)}, \"rich doc edit\")?.into()",
        )
    if kind == "rich_doc_create_request":
        return (
            "",
            f"from_value::<CreateRichDocJson>({owned_clone_expr(g)}, \"rich doc create\")?.into()",
        )
    if kind == "create_location_request":
        return ("", f"build_create_location_request({owned_clone_expr(g)})?")
    if kind == "create_sticker_request":
        return ("", f"build_create_sticker_request({owned_clone_expr(g)})?")
    if kind == "built_content_field":
        return (
            f"""let content = {g}.get("content").ok_or_else(|| crate::binding_invalid_parameter("missing content"))?;""",
            "built_content_from_value(content)?",
        )
    if kind == "json_null_default":
        base = g[1:] if g.startswith("&") else g
        return ("", f'{base}.get("payload").cloned().unwrap_or(serde_json::Value::Null)')
    if kind.startswith("str_any:"):
        keys = [wire_key(item) for item in kind.split(":", 1)[1].split(",")]
        v = f"{name}_s"
        keys_lit = ", ".join(json.dumps(k) for k in keys)
        return (f"let {v} = string_any({g}, &[{keys_lit}])?;", f"&{v}")
    if kind.startswith("optional_str_any:"):
        keys = [wire_key(item) for item in kind.split(":", 1)[1].split(",")]
        v = f"{name}_o"
        keys_lit = ", ".join(json.dumps(k) for k in keys)
        return (f"let {v} = optional_string_any({g}, &[{keys_lit}]);", f"{v}.as_deref()")
    if kind == "optional_i32_default":
        default = arg.get("default", 3600)
        v = f"{name}_v"
        return (
            f'let {v} = optional_i32({g}, "{key}").unwrap_or({default});',
            f"{v}",
        )
    if kind == "i32_u32":
        default = arg.get("default", 50)
        min_v = arg.get("min", 1)
        v = f"{name}_v"
        return (
            f'let {v} = json_i32({g}, "{key}").unwrap_or({default}).max({min_v}) as u32;',
            f"{v}",
        )
    if kind.startswith("str_ref_alt:"):
        alts = [wire_key(item) for item in kind.split(":", 1)[1].split(",")]
        v = f"{name}_s"
        inner = f"json_string({g}, {json.dumps(alts[-1])})"
        for alt in reversed(alts[:-1]):
            inner = f"json_string({g}, {json.dumps(alt)}).or_else(|_| {inner})"
        return (f"let {v} = {inner}?;", f"&{v}")
    if name == "literal" and kind == "bool":
        val = arg.get("value", False)
        return ("", "true" if val else "false")
    if name == "literal" and kind == "none":
        return ("", "None")
    if kind == "optional_upload_options":
        v = f"{name}_o"
        return (f"let {name}_o = optional_upload_options({g}, \"{key}\")?;", f"{name}_o")
    if kind == "bytes_vec":
        v = f"{name}_v"
        return (f'let {v} = json_bytes_vec({g}, "{key}")?;', f"{v}")
    if kind == "json_object":
        raise ValueError("json_object is result-only")
    raise ValueError(f"unsupported arg kind: {kind} for {name}")


def render_operation_body(op: dict[str, Any], group: dict[str, Any]) -> list[str]:
    value_expr = "&request" if group.get("op_from_request") else "&params"
    lets: list[str] = []
    call_args: list[str] = []
    for raw in op.get("args", []):
        arg = parse_arg(raw)
        if arg["name"] == "literal":
            if arg["kind"] == "bool":
                call_args.append("true" if arg.get("value") else "false")
            elif arg["kind"] == "none":
                call_args.append("None")
            continue
        if arg["name"] == "@value":
            let_stmt, expr = render_arg_extract(arg, group, value_expr)
            if let_stmt:
                lets.extend(let_stmt.split(";"))
                lets = [line for line in lets if line.strip()]
            call_args.append(expr)
            continue
        let_stmt, expr = render_arg_extract(arg, group, value_expr)
        if let_stmt:
            lets.extend(let_stmt.split(";"))
            lets = [line for line in lets if line.strip()]
        call_args.append(expr)

    receiver = "api"
    method = op["method"]
    result = op.get("result", "json")

    lines: list[str] = []
    for let_line in lets:
        if let_line.strip():
            lines.append(f"            {let_line.strip()};")

    if result == "unit":
        lines.append(f"            {receiver}.{method}({', '.join(call_args)}).await?;")
        lines.append("            Ok(BindingResponse::unit())")
    elif result == "send_ack":
        lines.append(f"            json_send_ack({receiver}.{method}({', '.join(call_args)}).await?)")
    elif result == "json_object":
        fields = op.get("fields", {})
        if len(fields) == 1:
            (key, var) = next(iter(fields.items()))
            lines.append(f"            let {var} = {receiver}.{method}({', '.join(call_args)}).await?;")
            lines.append(f'            json(serde_json::json!({{ "{key}": {var} }}))')
        else:
            lines.append(f"            json({receiver}.{method}({', '.join(call_args)}).await?)")
    else:
        lines.append(f"            json({receiver}.{method}({', '.join(call_args)}).await?)")
    return lines


def render_group(group: dict[str, Any]) -> str:
    gid = group["id"]
    recv = group["receiver"]
    extra = group.get("extra_receivers", [])
    dispatch_fn = group["dispatch_fn"]
    is_fn = group["is_fn"]
    ops_const = group["ops_const"]
    value_name = "request" if group.get("op_from_request") else "params"

    ops = group["operations"]
    native_ops = [op for op in ops if op.get("cfg") == 'not(target_arch = "wasm32")']
    common_ops = [op for op in ops if op.get("cfg") != 'not(target_arch = "wasm32")']

    lines = [
        "// @generated by bindings/contract/tools/dispatch_codegen.py",
        "// Do not edit by hand.",
        "#![allow(clippy::too_many_lines, unused_imports)]",
        "",
    ]

    if gid == "message":
        lines += [
            "use flare_im_core_sdk::client::api::MessageApi;",
            "use flare_im_core_sdk::model::MessageSearchQuery;",
        ]
    elif gid == "message_build":
        lines += [
            "use std::sync::Arc;",
            "use flare_im_core_sdk::client::api::MessageBuildApi;",
        ]
    elif gid == "conversation":
        lines += [
            "use flare_im_core_sdk::client::api::ConversationApi;",
            "use flare_im_core_sdk::model::{BootstrapHomeTimelineRequest, ConversationListQuery, OpenConversationTimelineRequest};",
        ]
    elif gid == "media":
        lines += ["use flare_im_core_sdk::client::api::MediaApi;"]
    elif gid == "capability":
        lines += [
            "use std::sync::Arc;",
            "use flare_im_core_sdk::client::IMClient;",
            "use flare_im_core_sdk::client::api::CapabilityApi;",
        ]

    lines += [
        "use serde_json::Value;",
        "use flare_im_core_sdk::Result;",
        "use crate::dispatch_support::*;",
        "use crate::{BindingResponse, binding_operation_not_supported};",
        "",
        f"pub const {ops_const}: &[&str] = &[",
    ]
    seen_ops: set[str] = set()
    for op in ops:
        for name in operation_names(op):
            if name in seen_ops:
                continue
            seen_ops.add(name)
            lines.append(f'    "{name}",')
    lines.append("];")
    lines.append("")
    lines.append(f"pub fn {is_fn}(operation: &str) -> bool {{")
    lines.append(f"    {ops_const}.contains(&operation)")
    lines.append("}")
    lines.append("")

    # native-only helper for media
    if native_ops and gid == "media":
        lines.append('#[cfg(not(target_arch = "wasm32"))]')
        lines.append(f"async fn {dispatch_fn}_native_only(")
        lines.append(f"    api: {recv['binding']},")
        lines.append("    operation: &str,")
        lines.append("    params: Value,")
        lines.append(") -> Result<BindingResponse> {")
        lines.append("    match operation {")
        for op in native_ops:
            lines.append(f'        {op_patterns(op)} => {{')
            lines.extend(render_operation_body(op, group))
            lines.append("        }")
        lines.append('        _ => Err(binding_operation_not_supported(operation)),')
        lines.append("    }")
        lines.append("}")
        lines.append("")

    # main dispatch
    sig_args = [f"api: {recv['binding']}"]
    for ex in extra:
        name = ex["name"]
        if group["id"] == "capability" and name == "client":
            name = "_client"
        sig_args.append(f"{name}: {ex['binding']}")
    if not group.get("op_from_request"):
        sig_args.append("operation: &str")
    sig_args.append(f"{value_name}: Value")
    lines.append(f"pub async fn {dispatch_fn}(")
    lines.append("    " + ",\n    ".join(sig_args) + ",")
    lines.append(") -> Result<BindingResponse> {")
    if group.get("op_from_request"):
        lines.append('    let operation = json_string(&request, "op")?;')

    if native_ops and gid == "media":
        native_pattern = " | ".join(f'"{op["op"]}"' for op in native_ops)
        lines.append('    #[cfg(not(target_arch = "wasm32"))]')
        lines.append(f"    if matches!(operation, {native_pattern}) {{")
        lines.append(f"        return {dispatch_fn}_native_only(api, operation, params).await;")
        lines.append("    }")
        lines.append("")

    match_expr = "operation.as_str()" if group.get("op_from_request") else "operation"
    lines.append(f"    match {match_expr} {{")
    for op in common_ops:
        if op.get("handler"):
            lines.append(f'        {op_patterns(op)} => {{')
            lines.extend(render_operation_body(op, group))
            lines.append("        }")
            continue
        lines.append(f'        {op_patterns(op)} => {{')
        lines.extend(render_operation_body(op, group))
        lines.append("        }")
    lines.append('        _ => Err(binding_operation_not_supported(operation)),')
    lines.append("    }")
    lines.append("}")
    lines.append("")
    return "\n".join(lines)


def render_dispatch_mod(groups: list[dict[str, Any]]) -> str:
    lines = [
        "// @generated by bindings/contract/tools/dispatch_codegen.py",
        *[f"pub mod {g['id']};" for g in groups],
        "",
    ]
    for g in groups:
        gid = g["id"]
        lines.append(f"pub use {gid}::{g['dispatch_fn']};")
        lines.append(f"pub use {gid}::{g['is_fn']};")
        lines.append(f"pub use {gid}::{g['ops_const']};")
    lines.append("")
    return "\n".join(lines)


def generate_all() -> dict[Path, str]:
    contract = load_dispatch()
    groups = contract["groups"]
    outputs: dict[Path, str] = {
        DISPATCH_OUT_DIR / "mod.rs": render_dispatch_mod(groups),
    }
    for group in groups:
        outputs[DISPATCH_OUT_DIR / f"{group['id']}.rs"] = render_group(group)
    return outputs


def write_outputs(outputs: dict[Path, str]) -> None:
    DISPATCH_OUT_DIR.mkdir(parents=True, exist_ok=True)
    for path, content in outputs.items():
        path.write_text(content, encoding="utf-8")
        print(f"generated {path}")
