#!/usr/bin/env python3
"""Generate typed C ABI shims from contract/c_typed_abi.json."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
CONTRACT_DIR = ROOT / "contract"
TYPED_ABI_JSON = CONTRACT_DIR / "c_typed_abi.json"
OUT = ROOT / "c" / "src" / "generated" / "typed_abi.rs"


def load_exports() -> list[dict[str, Any]]:
    with TYPED_ABI_JSON.open("r", encoding="utf-8") as file:
        data = json.load(file)
    exports = data.get("exports", [])
    if not isinstance(exports, list):
        raise ValueError(f"{TYPED_ABI_JSON} must contain exports array")
    return exports


def render_cstr_arg(param: str, label: str) -> str:
    msg = json.dumps(label, ensure_ascii=False)
    return f"""let {param} = match c_str_to_string({param}) {{
            Ok(s) => s,
            Err(code) => {{
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, {msg});
                return code;
            }}
        }};"""


def render_upload_options_prelude(name: str, _key: str) -> str:
    return f"""let upload_options = match parse_upload_options({name}) {{
            Ok(v) => v,
            Err(code) => {{
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, code, "Invalid upload options");
                return code;
            }}
        }};
        let upload_options_value = match upload_options {{
            Some(opts) => serde_json::json!({{ "chunk_size": opts.chunk_size }}),
            None => serde_json::Value::Null,
        }};"""


def render_arg_parse(arg: dict[str, Any], *, with_callback: bool) -> tuple[str, list[str]]:
    name = arg["name"]
    atype = arg["type"]
    lines: list[str] = []
    ctx_setup = "let ctx = CallbackContext::new(context, callback);" if with_callback else ""

    if atype == "c_str":
        if with_callback:
            return f"{name}: *const c_char", [render_cstr_arg(name, f"Invalid {name}")]
        msg = json.dumps(f"Invalid {name}", ensure_ascii=False)
        return (
            f"{name}: *const c_char",
            [
                f"""let {name} = match c_str_to_string({name}) {{
            Ok(s) => s,
            Err(_) => return false,
        }};"""
            ],
        )
    if atype == "u64":
        return f"{name}: u64", []
    if atype == "i32":
        return f"{name}: i32", []
    if atype == "bool":
        return f"{name}: bool", []
    if atype == "upload_options":
        return f"{name}: *const c_char", [render_upload_options_prelude(name, arg.get("json_key", "options"))]
    if atype == "bytes_view":
        return (
            f"{name}: FlareBytesView",
            [
                f"""if {name}.ptr.is_null() || {name}.len == 0 {{
            {ctx_setup}
            return_error(&ctx, crate::error_convert::FLARE_ERR_INVALID_PARAM, "Invalid bytes");
            return crate::error_convert::FLARE_ERR_INVALID_PARAM;
        }}
        let payload = unsafe {{ std::slice::from_raw_parts({name}.ptr, {name}.len) }}.to_vec();"""
            ],
        )
    if atype == "request_json":
        return (
            f"{name}: *const c_char",
            [
                f"""let params: serde_json::Value = match parse_json({name}) {{
            Ok(v) => v,
            Err(code) => {{
                {ctx_setup}
                return_error(&ctx, code, "Invalid request JSON");
                return code;
            }}
        }};"""
            ],
        )
    if atype == "json_vec":
        lines.append(render_cstr_arg(name, f"Invalid {name}"))
        lines.append(
            f"""let {name} = match serde_json::from_str::<Vec<String>>(&{name}) {{
            Ok(v) => v,
            Err(_) => {{
                {ctx_setup}
                return_error(&ctx, crate::error_convert::FLARE_ERR_INVALID_PARAM, "Invalid user_ids_json");
                return crate::error_convert::FLARE_ERR_INVALID_PARAM;
            }}
        }};"""
        )
        return f"{name}: *const c_char", lines
    if atype == "json_message":
        lines.append(
            f"""let message: flare_im_core_sdk::model::IMMessage = match parse_json({name}) {{
            Ok(m) => m,
            Err(code) => {{
                {ctx_setup}
                return_error(&ctx, code, "Invalid message JSON");
                return code;
            }}
        }};"""
        )
        return f"{name}: *const c_char", lines
    raise ValueError(f"unsupported arg type: {atype}")


def render_params_build(args: list[dict[str, Any]]) -> list[str]:
    if not args:
        return ["let params = serde_json::Value::Null;"]
    if len(args) == 1 and args[0]["type"] == "request_json":
        return []

    prelude: list[str] = []
    fields: list[str] = []
    for arg in args:
        key = arg.get("json_key", arg["name"])
        name = arg["name"]
        atype = arg["type"]
        if atype == "request_json":
            continue
        if atype == "json_message":
            prelude.append(
                """let message_value = match serde_json::to_value(&message) {
            Ok(v) => v,
            Err(_) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, crate::error_convert::FLARE_ERR_INVALID_PARAM, "Invalid message");
                return crate::error_convert::FLARE_ERR_INVALID_PARAM;
            }
        };"""
            )
            fields.append(f'"{key}": message_value')
        elif atype == "json_vec":
            prelude.append(
                f"""let {name}_value = match serde_json::to_value(&{name}) {{
            Ok(v) => v,
            Err(_) => {{
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, crate::error_convert::FLARE_ERR_INVALID_PARAM, "Invalid user_ids");
                return crate::error_convert::FLARE_ERR_INVALID_PARAM;
            }}
        }};"""
            )
            fields.append(f'"{key}": {name}_value')
        elif atype == "upload_options":
            fields.append(f'"{key}": upload_options_value')
        elif atype == "bytes_view":
            prelude.append(
                """let bytes_value = match serde_json::to_value(&payload) {
            Ok(v) => v,
            Err(_) => {
                let ctx = CallbackContext::new(context, callback);
                return_error(&ctx, crate::error_convert::FLARE_ERR_INVALID_PARAM, "Invalid bytes");
                return crate::error_convert::FLARE_ERR_INVALID_PARAM;
            }
        };"""
            )
            fields.append(f'"{key}": bytes_value')
        elif atype == "bool":
            fields.append(f'"{key}": {name}')
        else:
            fields.append(f'"{key}": {name}')

    prelude.append(f"let params = serde_json::json!({{{', '.join(fields)}}});")
    return prelude


def render_sync_bool_invoke(entry: dict[str, Any]) -> str:
    symbol = entry["symbol"]
    api_id = entry["api_id"]
    args = entry.get("args", [])
    arg_decls: list[str] = []
    parse_lines: list[str] = []
    for arg in args:
        decl, lines = render_arg_parse(arg, with_callback=False)
        arg_decls.append(decl)
        parse_lines.extend(lines)

    params_lines = render_params_build(args)
    if not (len(args) == 1 and args[0]["type"] == "request_json"):
        params_block = "\n        ".join(parse_lines + params_lines)
        params_tail = ""
    else:
        params_block = "\n        ".join(parse_lines)
        params_tail = ""

    arg_section = ""
    if arg_decls:
        arg_section = "    " + ",\n    ".join(arg_decls) + ",\n"

    return f"""
#[unsafe(no_mangle)]
pub extern "C" fn {symbol}(
    handle: FlareHandle,
{arg_section}) -> bool {{
    abi::catch_ffi_bool(|| {{
        let instance = match require_instance(handle) {{
            Ok(i) => i,
            Err(_) => return false,
        }};
        {params_block}
        {params_tail}
        let api_id = {json.dumps(api_id)};
        let inst = instance.clone();
        let response = match instance.runtime.block_on(async move {{
            invoke_api_id(inst.as_ref(), &api_id, params).await
        }}) {{
            Ok(v) => v,
            Err(_) => return false,
        }};
        response
            .payload
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }})
}}
"""


def render_export(entry: dict[str, Any]) -> str:
    symbol = entry["symbol"]
    kind = entry["kind"]
    if kind == "sync_i32":
        return f"""
#[unsafe(no_mangle)]
pub extern "C" fn {symbol}(handle: FlareHandle) -> i32 {{
    abi::catch_ffi_i32(|| sdk_state_code(handle))
}}
"""
    if kind == "sync_bool_invoke":
        return render_sync_bool_invoke(entry)

    args = entry.get("args", [])
    api_id = entry["api_id"]
    arg_decls: list[str] = []
    parse_lines: list[str] = []
    for arg in args:
        decl, lines = render_arg_parse(arg, with_callback=True)
        arg_decls.append(decl)
        parse_lines.extend(lines)

    params_lines = render_params_build(args)
    invoke_fn = {
        "invoke_unit": "typed_invoke_unit",
        "invoke_json": "typed_invoke_json",
        "invoke_send_ack": "typed_invoke_send_ack",
    }[kind]

    arg_section = ""
    if arg_decls:
        arg_section = "    " + ",\n    ".join(arg_decls) + ",\n"

    body = "\n        ".join(parse_lines + params_lines)
    return f"""
#[unsafe(no_mangle)]
pub extern "C" fn {symbol}(
    handle: FlareHandle,
{arg_section}    context: *mut c_void,
    callback: FlareResultCallback,
) -> i32 {{
    abi::catch_ffi_i32(|| {{
        let instance = match require_instance(handle) {{
            Ok(i) => i,
            Err(e) => return e,
        }};
        {body}
        let ctx = CallbackContext::new(context, callback);
        let api_id = {json.dumps(api_id)};
        {invoke_fn}(instance, ctx, &api_id, params);
        0
    }})
}}
"""


def render_module(exports: list[dict[str, Any]]) -> str:
    return f"""// @generated by bindings/contract/tools/c_abi_codegen.py
// Source: bindings/contract/c_typed_abi.json

use std::ffi::{{c_char, c_void}};

use crate::abi;
use crate::dispatch_common::{{typed_invoke_json, typed_invoke_send_ack, typed_invoke_unit}};
use crate::executor::{{CallbackContext, return_error}};
use crate::helpers::{{c_str_to_string, parse_json, parse_upload_options}};
use crate::registry::{{require_instance, retain_instance}};
use crate::types::{{FlareBytesView, FlareHandle, FlareResultCallback}};
use flare_im_core_sdk_bindings_runtime::invoke_api_id;

#[repr(i32)]
#[derive(Clone, Copy)]
enum FlareSdkStateCode {{
    Disconnected = 0,
    Connecting = 1,
    Connected = 2,
    Ready = 3,
    Reconnecting = 4,
}}

fn map_sdk_state(s: flare_im_core_sdk::core::SdkState) -> i32 {{
    use flare_im_core_sdk::core::SdkState as S;
    match s {{
        S::Disconnected => FlareSdkStateCode::Disconnected as i32,
        S::Connecting => FlareSdkStateCode::Connecting as i32,
        S::Connected => FlareSdkStateCode::Connected as i32,
        S::Ready => FlareSdkStateCode::Ready as i32,
        S::Reconnecting => FlareSdkStateCode::Reconnecting as i32,
    }}
}}

fn sdk_state_code(handle: FlareHandle) -> i32 {{
    retain_instance(handle).map_or(FlareSdkStateCode::Disconnected as i32, |instance| {{
        map_sdk_state(instance.client.state())
    }})
}}
{"".join(render_export(e) for e in exports)}
"""


def generate_all() -> dict[Path, str]:
    return {OUT: render_module(load_exports())}
