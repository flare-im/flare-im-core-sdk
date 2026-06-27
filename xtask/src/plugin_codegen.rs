use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flare_im_core_sdk::plugin::SdkPluginManifest;
use schemars::schema_for;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub(crate) fn run(sdk_root: &Path, command: &str) -> Result<()> {
    match command {
        "verify" => {
            verify(sdk_root)?;
            println!("plugin manifest verify passed");
            Ok(())
        }
        "schema" => write_schema(sdk_root, false),
        "schema-check" => write_schema(sdk_root, true),
        "codegen" => {
            verify(sdk_root)?;
            write_schema(sdk_root, false)?;
            write_stubs(sdk_root, false)
        }
        "check" => {
            verify(sdk_root)?;
            write_schema(sdk_root, true)?;
            write_stubs(sdk_root, true)
        }
        other => bail!("unknown plugin xtask command: {other}"),
    }
}

fn verify(sdk_root: &Path) -> Result<Vec<PluginManifestFile>> {
    let plugin_root = plugin_root(sdk_root)?;
    let manifests = load_plugin_manifests(&plugin_root)?;
    if manifests.is_empty() {
        bail!(
            "no plugin manifests found under {}; expected at least one plugin.json",
            plugin_root.display()
        );
    }

    let mut ids = BTreeSet::new();
    let mut namespaces = BTreeMap::<String, String>::new();
    for item in &manifests {
        item.manifest
            .validate(&item.manifest.id, &item.manifest.namespaces)
            .map_err(|error| anyhow::anyhow!("{error}"))
            .with_context(|| format!("invalid plugin manifest {}", item.path.display()))?;
        if !ids.insert(item.manifest.id.clone()) {
            bail!("duplicate plugin manifest id {}", item.manifest.id);
        }
        for namespace in &item.manifest.namespaces {
            if let Some(owner) = namespaces.insert(namespace.to_string(), item.manifest.id.clone())
            {
                bail!(
                    "plugin namespace {namespace} is claimed by both {owner} and {}",
                    item.manifest.id
                );
            }
        }
    }

    verify_catalog(&plugin_root, &manifests)?;
    Ok(manifests)
}

fn write_schema(sdk_root: &Path, check: bool) -> Result<()> {
    let plugin_root = plugin_root(sdk_root)?;
    let schema = schema_for!(SdkPluginManifest);
    let content = format!("{}\n", serde_json::to_string_pretty(&schema)?);
    write_text(
        &plugin_root.join("schema/plugin_manifest.schema.json"),
        &content,
        check,
    )
}

fn write_stubs(sdk_root: &Path, check: bool) -> Result<()> {
    let manifests = verify(sdk_root)?;
    let plugin_root = plugin_root(sdk_root)?;
    for item in manifests {
        let plugin_dir = generated_dir(&plugin_root, &item.manifest.id);
        for output in stub_outputs(&item.manifest) {
            write_text(&plugin_dir.join(output.path), &output.content, check)?;
        }
    }
    Ok(())
}

fn verify_catalog(plugin_root: &Path, manifests: &[PluginManifestFile]) -> Result<()> {
    let path = plugin_root.join("catalog/plugins.json");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read plugin catalog {}", path.display()))?;
    let catalog: PluginCatalog = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse plugin catalog {}", path.display()))?;
    if catalog.catalog_version.trim().is_empty() {
        bail!("plugin catalog catalogVersion must not be empty");
    }
    if catalog.schema.trim().is_empty() {
        bail!("plugin catalog schema must not be empty");
    }

    let by_id = manifests
        .iter()
        .map(|item| (item.manifest.id.as_str(), item))
        .collect::<BTreeMap<_, _>>();
    let mut catalog_ids = BTreeSet::new();
    for entry in &catalog.plugins {
        if !catalog_ids.insert(entry.id.as_str()) {
            bail!("duplicate plugin catalog id {}", entry.id);
        }
        let Some(item) = by_id.get(entry.id.as_str()) else {
            bail!("catalog references missing plugin manifest {}", entry.id);
        };
        if entry.version != item.manifest.version {
            bail!(
                "catalog entry {} version {} does not match manifest version {}",
                entry.id,
                entry.version,
                item.manifest.version
            );
        }
        if entry.display_name != item.manifest.display_name {
            bail!(
                "catalog entry {} displayName does not match manifest displayName",
                entry.id
            );
        }
        if entry.manifest_path.trim().is_empty() || entry.source_path.trim().is_empty() {
            bail!("catalog entry {} paths must not be empty", entry.id);
        }
        let expected = normalize_relative_path(plugin_root, &item.path)?;
        if entry.manifest_path != expected {
            bail!(
                "catalog entry {} manifestPath {} does not match {}",
                entry.id,
                entry.manifest_path,
                expected
            );
        }
        let Some(expected_source_path) = expected.strip_suffix("/plugin.json") else {
            bail!(
                "catalog entry {} manifestPath must point to plugin.json",
                entry.id
            );
        };
        if entry.source_path != expected_source_path {
            bail!(
                "catalog entry {} sourcePath {} must match manifest directory {}",
                entry.id,
                entry.source_path,
                expected_source_path
            );
        }
        let expected_category = item
            .manifest
            .namespaces
            .first()
            .expect("validated manifest has at least one namespace");
        if entry.category != *expected_category {
            bail!(
                "catalog entry {} category {} must match manifest namespace {}",
                entry.id,
                entry.category,
                expected_category
            );
        }
        let manifest_sha256 = sha256_file(&item.path)?;
        if entry.manifest_sha256 != manifest_sha256 {
            bail!(
                "catalog entry {} manifestSha256 {} does not match {}",
                entry.id,
                entry.manifest_sha256,
                manifest_sha256
            );
        }
        if entry.platforms != item.manifest.platforms {
            bail!(
                "catalog entry {} platforms must match manifest platforms",
                entry.id
            );
        }
        if entry.distribution != "composition-time" {
            bail!(
                "catalog entry {} distribution must be composition-time",
                entry.id,
            );
        }
        if entry.runtime_install != "not-supported" {
            bail!(
                "catalog entry {} runtimeInstall must be not-supported",
                entry.id,
            );
        }
    }

    for item in manifests {
        if !catalog_ids.contains(item.manifest.id.as_str()) {
            bail!(
                "plugin manifest {} is missing from catalog",
                item.manifest.id
            );
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "failed to read plugin manifest for sha256 {}",
            path.display()
        )
    })?;
    let digest = Sha256::digest(&bytes);
    Ok(format!("{digest:x}"))
}

fn load_plugin_manifests(plugin_root: &Path) -> Result<Vec<PluginManifestFile>> {
    let mut paths = Vec::new();
    collect_plugin_manifest_paths(plugin_root, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read plugin manifest {}", path.display()))?;
            let manifest = serde_json::from_str::<SdkPluginManifest>(&raw)
                .with_context(|| format!("failed to parse plugin manifest {}", path.display()))?;
            Ok(PluginManifestFile { path, manifest })
        })
        .collect()
}

fn collect_plugin_manifest_paths(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let skip_dirs = ["target", "generated", "schema", "catalog", "template"];
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry =
            entry.with_context(|| format!("failed to read entry under {}", dir.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if entry.file_type()?.is_dir() {
            if skip_dirs.contains(&file_name.as_ref()) {
                continue;
            }
            collect_plugin_manifest_paths(&path, out)?;
        } else if file_name == "plugin.json" {
            out.push(path);
        }
    }
    Ok(())
}

fn stub_outputs(manifest: &SdkPluginManifest) -> Vec<GeneratedStub> {
    let class_name = format!("{}PluginApi", pascal_identifier(&manifest.id));
    vec![
        GeneratedStub {
            path: "typescript/pluginApi.ts".into(),
            content: typescript_stub(manifest, &class_name),
        },
        GeneratedStub {
            path: format!("android/{class_name}.kt").into(),
            content: kotlin_stub(manifest, &class_name),
        },
        GeneratedStub {
            path: format!("ios/{class_name}.swift").into(),
            content: swift_stub(manifest, &class_name),
        },
        GeneratedStub {
            path: format!("flutter/{}_plugin_api.dart", snake_identifier(&manifest.id)).into(),
            content: dart_stub(manifest, &class_name),
        },
        GeneratedStub {
            path: format!("harmonyos-arkts/{class_name}.ets").into(),
            content: arkts_stub(manifest, &class_name),
        },
        GeneratedStub {
            path: format!("cangjie/{class_name}.cj").into(),
            content: cangjie_stub(manifest, &class_name),
        },
    ]
}

fn typescript_stub(manifest: &SdkPluginManifest, class_name: &str) -> String {
    let mut methods = String::new();
    for operation in &manifest.operations {
        let method = camel_identifier(&operation.op);
        let capability_id = capability_id(manifest, &operation.op);
        methods.push_str(&format!(
            "  async {method}(request: FlarePluginDispatchInput = {{}}): Promise<FlareJsonObject> {{\n    return this.capabilities.dispatchCapability({{\n      capabilityId: '{capability_id}',\n      payload: request.payload ?? {{}},\n      conversationId: request.conversationId,\n      tenantId: request.tenantId,\n      userId: request.userId,\n    }});\n  }}\n\n"
        ));
    }
    format!(
        r#"// GENERATED by `cargo xtask plugin-codegen`. Do not edit by hand.
export type FlareJsonObject = Record<string, unknown>;

export interface FlarePluginDispatchInput {{
  payload?: unknown;
  conversationId?: string;
  tenantId?: string;
  userId?: string;
}}

export interface FlareCapabilityDispatchPort {{
  dispatchCapability(request: FlareJsonObject): Promise<FlareJsonObject>;
}}

export class {class_name} {{
  constructor(private readonly capabilities: FlareCapabilityDispatchPort) {{}}

{methods}}}
"#
    )
}

fn kotlin_stub(manifest: &SdkPluginManifest, class_name: &str) -> String {
    let package = format!(
        "com.flare.im.plugin.{}",
        snake_identifier(&manifest.id).replace('_', "")
    );
    let mut methods = String::new();
    for operation in &manifest.operations {
        let method = camel_identifier(&operation.op);
        let capability_id = capability_id(manifest, &operation.op);
        methods.push_str(&format!(
            "    suspend fun {method}(\n        payload: Map<String, Any?> = emptyMap(),\n        conversationId: String? = null,\n        tenantId: String? = null,\n        userId: String? = null,\n    ): Map<String, Any?> = capabilities.dispatchCapability(\n        mapOf(\n            \"capabilityId\" to \"{capability_id}\",\n            \"payload\" to payload,\n            \"conversationId\" to conversationId,\n            \"tenantId\" to tenantId,\n            \"userId\" to userId,\n        ).filterValues {{ it != null }}\n    )\n\n"
        ));
    }
    format!(
        r#"package {package}

// GENERATED by `cargo xtask plugin-codegen`. Do not edit by hand.
interface CapabilityDispatchPort {{
    suspend fun dispatchCapability(request: Map<String, Any?>): Map<String, Any?>
}}

class {class_name}(private val capabilities: CapabilityDispatchPort) {{
{methods}}}
"#
    )
}

fn swift_stub(manifest: &SdkPluginManifest, class_name: &str) -> String {
    let mut methods = String::new();
    for operation in &manifest.operations {
        let method = camel_identifier(&operation.op);
        let capability_id = capability_id(manifest, &operation.op);
        methods.push_str(&format!(
            "    public func {method}(\n        payload: [String: AnySendable] = [:],\n        conversationId: String? = nil,\n        tenantId: String? = nil,\n        userId: String? = nil\n    ) async throws -> [String: AnySendable] {{\n        var request: [String: AnySendable] = [\n            \"capabilityId\": AnySendable(\"{capability_id}\"),\n            \"payload\": AnySendable(payload.mapValues {{ $0.value }})\n        ]\n        if let conversationId {{ request[\"conversationId\"] = AnySendable(conversationId) }}\n        if let tenantId {{ request[\"tenantId\"] = AnySendable(tenantId) }}\n        if let userId {{ request[\"userId\"] = AnySendable(userId) }}\n        return try await capabilities.dispatchCapability(request)\n    }}\n\n"
        ));
    }
    format!(
        r#"import Foundation

// GENERATED by `cargo xtask plugin-codegen`. Do not edit by hand.
public protocol CapabilityDispatchPort: AnyObject {{
    func dispatchCapability(_ request: [String: AnySendable]) async throws -> [String: AnySendable]
}}

public final class {class_name} {{
    private let capabilities: any CapabilityDispatchPort

    public init(capabilities: any CapabilityDispatchPort) {{
        self.capabilities = capabilities
    }}

{methods}}}
"#
    )
}

fn dart_stub(manifest: &SdkPluginManifest, class_name: &str) -> String {
    let mut methods = String::new();
    for operation in &manifest.operations {
        let method = camel_identifier(&operation.op);
        let capability_id = capability_id(manifest, &operation.op);
        methods.push_str(&format!(
            "  Future<Map<String, Object?>> {method}({{\n    Map<String, Object?> payload = const {{}},\n    String? conversationId,\n    String? tenantId,\n    String? userId,\n  }}) {{\n    final request = <String, Object?>{{\n      'capabilityId': '{capability_id}',\n      'payload': payload,\n      'conversationId': conversationId,\n      'tenantId': tenantId,\n      'userId': userId,\n    }}..removeWhere((_, value) => value == null);\n    return capabilities.dispatchCapability(request);\n  }}\n\n"
        ));
    }
    format!(
        r#"// GENERATED by `cargo xtask plugin-codegen`. Do not edit by hand.
abstract interface class CapabilityDispatchPort {{
  Future<Map<String, Object?>> dispatchCapability(Map<String, Object?> request);
}}

final class {class_name} {{
  {class_name}(this.capabilities);

  final CapabilityDispatchPort capabilities;

{methods}}}
"#
    )
}

fn arkts_stub(manifest: &SdkPluginManifest, class_name: &str) -> String {
    let mut methods = String::new();
    for operation in &manifest.operations {
        let method = camel_identifier(&operation.op);
        let capability_id = capability_id(manifest, &operation.op);
        methods.push_str(&format!(
            "  async {method}(input: FlarePluginDispatchInput = {{}}): Promise<Record<string, Object>> {{\n    return await this.capabilities.dispatchCapability({{\n      capabilityId: '{capability_id}',\n      payload: input.payload ?? {{}},\n      conversationId: input.conversationId,\n      tenantId: input.tenantId,\n      userId: input.userId,\n    }});\n  }}\n\n"
        ));
    }
    format!(
        r#"// GENERATED by `cargo xtask plugin-codegen`. Do not edit by hand.
export interface FlarePluginDispatchInput {{
  payload?: Object;
  conversationId?: string;
  tenantId?: string;
  userId?: string;
}}

export interface CapabilityDispatchPort {{
  dispatchCapability(request: Record<string, Object>): Promise<Record<string, Object>>;
}}

export class {class_name} {{
  constructor(private readonly capabilities: CapabilityDispatchPort) {{}}

{methods}}}
"#
    )
}

fn cangjie_stub(manifest: &SdkPluginManifest, class_name: &str) -> String {
    let mut methods = String::new();
    for operation in &manifest.operations {
        let method = camel_identifier(&operation.op);
        let capability_id = capability_id(manifest, &operation.op);
        methods.push_str(&format!(
            "    // Capability: {capability_id}. requestJson must contain capabilityId/payload/conversationId/tenantId/userId.\n    public func {method}(requestJson: String): String {{\n        return capabilities.dispatchCapability(requestJson: requestJson)\n    }}\n\n"
        ));
    }
    format!(
        r#"package flare_core_harmony_cangjie_sdk.plugin

import flare_core_harmony_cangjie_sdk.api.modules.*

// GENERATED by `cargo xtask plugin-codegen`. Do not edit by hand.
public class {class_name} {{
    private let capabilities: CapabilitiesApi

    public init(capabilities: CapabilitiesApi) {{
        this.capabilities = capabilities
    }}

{methods}}}
"#
    )
}

fn capability_id(manifest: &SdkPluginManifest, op: &str) -> String {
    if manifest
        .namespaces
        .iter()
        .any(|namespace| op == namespace || op.starts_with(&format!("{namespace}.")))
    {
        op.to_string()
    } else {
        format!("{}.{}", manifest.namespaces[0], op)
    }
}

fn camel_identifier(input: &str) -> String {
    let pascal = pascal_identifier(input);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => format!(
            "{}{}",
            first.to_ascii_lowercase(),
            chars.collect::<String>()
        ),
        None => "operation".to_string(),
    }
}

fn pascal_identifier(input: &str) -> String {
    let mut output = String::new();
    for part in input.split(|c: char| !c.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.push(first.to_ascii_uppercase());
            output.push_str(chars.as_str());
        }
    }
    if output.is_empty() {
        "Plugin".to_string()
    } else if output.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("Plugin{output}")
    } else {
        output
    }
}

fn snake_identifier(input: &str) -> String {
    let mut output = String::new();
    for (idx, part) in input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .enumerate()
    {
        if idx > 0 {
            output.push('_');
        }
        output.push_str(&part.to_ascii_lowercase());
    }
    if output.is_empty() {
        "plugin".to_string()
    } else if output.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("plugin_{output}")
    } else {
        output
    }
}

fn generated_dir(plugin_root: &Path, plugin_id: &str) -> PathBuf {
    plugin_root
        .join("generated")
        .join(snake_identifier(plugin_id))
}

fn plugin_root(sdk_root: &Path) -> Result<PathBuf> {
    let repo_root = sdk_root
        .parent()
        .context("flare-im-core-sdk must live under repository root")?;
    Ok(repo_root.join("flare-sdk-plugin"))
}

fn normalize_relative_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn write_text(path: &Path, content: &str, check: bool) -> Result<()> {
    if check {
        let existing = fs::read_to_string(path)
            .with_context(|| format!("generated file missing: {}", path.display()))?;
        if existing != content {
            bail!("generated file drift: {}", path.display());
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if fs::read_to_string(path).ok().as_deref() != Some(content) {
        fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

struct PluginManifestFile {
    path: PathBuf,
    manifest: SdkPluginManifest,
}

struct GeneratedStub {
    path: PathBuf,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCatalog {
    catalog_version: String,
    schema: String,
    plugins: Vec<PluginCatalogEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginCatalogEntry {
    id: String,
    version: String,
    display_name: String,
    manifest_path: String,
    manifest_sha256: String,
    source_path: String,
    category: String,
    distribution: String,
    runtime_install: String,
    platforms: Vec<String>,
}
