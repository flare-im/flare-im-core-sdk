use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn emit_wire_boundaries(root: &Path, check: bool) -> Result<()> {
    let mut drifted = Vec::new();
    for target in wire_boundary_targets(root) {
        upsert_wire_boundary(&target, check, &mut drifted)?;
    }
    if !drifted.is_empty() {
        let details = drifted.join("\n  - ");
        bail!("Rust-owned wire boundary output drifted:\n  - {details}");
    }
    if !check {
        println!("Rust-owned wire boundary artifacts generated");
    }
    Ok(())
}

pub(crate) struct WireBoundaryTarget {
    pub(crate) path: PathBuf,
    section: &'static str,
    legacy_sections: &'static [&'static str],
}

pub(crate) fn wire_boundary_targets(root: &Path) -> Vec<WireBoundaryTarget> {
    vec![
        WireBoundaryTarget {
            path: root.join("packages/flare-core-typescript-sdk/src/adapter/codec/wireCodec.ts"),
            section: TS_WIRE_BOUNDARY,
            legacy_sections: &[LEGACY_TS_WIRE_BOUNDARY],
        },
        WireBoundaryTarget {
            path: root.join(
                "packages/flare-core-android-sdk/src/main/kotlin/com/flare/im/adapter/codec/WireCodec.kt",
            ),
            section: KOTLIN_WIRE_BOUNDARY,
            legacy_sections: &[LEGACY_KOTLIN_WIRE_BOUNDARY],
        },
        WireBoundaryTarget {
            path: root.join(
                "packages/flare-core-apple-sdk/Sources/FlareCoreAppleSDK/Bridge/FfiBridgeSupport.swift",
            ),
            section: SWIFT_WIRE_BOUNDARY,
            legacy_sections: &[LEGACY_SWIFT_WIRE_BOUNDARY],
        },
        WireBoundaryTarget {
            path: root.join("packages/flare-core-flutter-sdk/lib/src/adapter/codec/wire_codec.dart"),
            section: DART_WIRE_BOUNDARY,
            legacy_sections: &[LEGACY_DART_WIRE_BOUNDARY],
        },
        WireBoundaryTarget {
            path: root.join(
                "packages/flare-core-harmony-arkts-sdk/src/main/ets/adapter/codec/WireCodec.ets",
            ),
            section: ARKTS_WIRE_BOUNDARY,
            legacy_sections: &[LEGACY_ARKTS_WIRE_BOUNDARY],
        },
        WireBoundaryTarget {
            path: root.join("packages/flare-core-harmony-cangjie-sdk/src/adapter/codec/WireCodec.cj"),
            section: CANGJIE_WIRE_BOUNDARY,
            legacy_sections: &[LEGACY_CANGJIE_WIRE_BOUNDARY],
        },
    ]
}

fn upsert_wire_boundary(
    target: &WireBoundaryTarget,
    check: bool,
    drifted: &mut Vec<String>,
) -> Result<()> {
    let original = fs::read_to_string(&target.path)
        .with_context(|| format!("failed to read {}", target.path.display()))?;
    let mut next = remove_marked_wire_boundary(&original)?;
    for legacy in target.legacy_sections {
        next = next.replace(legacy, "");
    }
    let section = rust_owned_wire_boundary_section(target.section);
    next = format!("{}\n\n{}\n", next.trim_end(), section.trim_end());
    if next == original {
        return Ok(());
    }
    if check {
        drifted.push(target.path.display().to_string());
    } else {
        fs::write(&target.path, next)
            .with_context(|| format!("failed to write {}", target.path.display()))?;
    }
    Ok(())
}

fn remove_marked_wire_boundary(body: &str) -> Result<String> {
    let Some(begin) = body.find(WIRE_BOUNDARY_BEGIN) else {
        return Ok(body.to_string());
    };
    let Some(end) = body[begin..]
        .find(WIRE_BOUNDARY_END)
        .map(|offset| begin + offset)
    else {
        bail!("wire boundary begin marker exists without end marker");
    };
    let end = end + WIRE_BOUNDARY_END.len();
    let mut next = String::with_capacity(body.len());
    next.push_str(&body[..begin]);
    next.push_str(&body[end..]);
    Ok(next)
}

fn rust_owned_wire_boundary_section(body: &str) -> String {
    format!("{WIRE_BOUNDARY_BEGIN}\n{body}\n{WIRE_BOUNDARY_END}")
}

const WIRE_BOUNDARY_BEGIN: &str = "// RUST-OWNED WIRE BOUNDARY: BEGIN";
const WIRE_BOUNDARY_END: &str = "// RUST-OWNED WIRE BOUNDARY: END";

const TS_WIRE_BOUNDARY: &str = r#"/** The FFI wire contract is canonical camelCase SDK JSON. */
export function wireEncodeRequest(value: unknown): unknown {
  return value;
}

/** The FFI wire contract is canonical camelCase SDK JSON. */
export function wireDecodeResponse(value: unknown): unknown {
  return value;
}"#;

const KOTLIN_WIRE_BOUNDARY: &str = r#"/** The FFI wire contract is canonical camelCase SDK JSON. */
fun wireEncodeRequest(value: Any?): Any? = value

/** The FFI wire contract is canonical camelCase SDK JSON. */
fun wireDecodeResponse(value: Any?): Any? = value"#;

const SWIFT_WIRE_BOUNDARY: &str = r#"enum FfiWireBoundary {
    static func encodeRequest(_ value: Any?) -> Any? {
        return value
    }

    static func decodeResponse(_ value: Any?) -> Any? {
        return value
    }
}"#;

const DART_WIRE_BOUNDARY: &str = r#"/// The FFI wire contract is canonical camelCase SDK JSON.
Object? wireEncodeRequest(Object? value) {
  return value;
}

/// The FFI wire contract is canonical camelCase SDK JSON.
Object? wireDecodeResponse(Object? value) {
  return value;
}"#;

const ARKTS_WIRE_BOUNDARY: &str = r#"/** The FFI wire contract is canonical camelCase SDK JSON. */
export function wireEncodeRequest(value: unknown): unknown {
  return value;
}

/** The FFI wire contract is canonical camelCase SDK JSON. */
export function wireDecodeResponse(value: unknown): unknown {
  return value;
}"#;

const CANGJIE_WIRE_BOUNDARY: &str = r#"public func wireEncodeRequest(raw: String): String {
    return raw
}

public func wireDecodeResponse(raw: String): String {
    return raw
}

public func wireDecodeResponseObject(object: CjJsonObject): CjJsonObject {
    return object
}"#;

const LEGACY_TS_WIRE_BOUNDARY: &str = r#"/** The FFI wire contract is canonical camelCase SDK JSON. */
export function wireEncodeRequest(value: unknown): unknown {
  return value;
}

function normalizeWireResponseValue(value: unknown): unknown {
  if (value instanceof Map) {
    return Object.fromEntries(
      Array.from(value.entries()).map(([key, item]) => [String(key), normalizeWireResponseValue(item)]),
    );
  }
  if (Array.isArray(value)) {
    return value.map((item) => normalizeWireResponseValue(item));
  }
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, item]) => [key, normalizeWireResponseValue(item)]),
    );
  }
  return value;
}

/** The FFI wire contract is canonical camelCase SDK JSON. */
export function wireDecodeResponse(value: unknown): unknown {
  return normalizeWireResponseValue(value);
}"#;
const LEGACY_KOTLIN_WIRE_BOUNDARY: &str = KOTLIN_WIRE_BOUNDARY;
const LEGACY_SWIFT_WIRE_BOUNDARY: &str = SWIFT_WIRE_BOUNDARY;
const LEGACY_DART_WIRE_BOUNDARY: &str = r#"Object? wireEncodeRequest(Object? value) => value;

Object? wireDecodeResponse(Object? value) => value;"#;
const LEGACY_ARKTS_WIRE_BOUNDARY: &str = ARKTS_WIRE_BOUNDARY;
const LEGACY_CANGJIE_WIRE_BOUNDARY: &str = r#"import encoding.json.JsonObject as JsonObject

public func wireEncodeRequest(raw: String): String {
    if (raw.size == 0) {
        return "{}"
    }
    return raw
}

public func wireDecodeResponse(raw: String): String {
    if (raw.size == 0) {
        return "{}"
    }
    return raw
}

public func wireDecodeResponseObject(object: CjJsonObject): CjJsonObject {
    return object
}"#;
