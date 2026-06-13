use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map(String::as_str).unwrap_or("verify");
    let root = workspace_root()?;
    let contracts = Contracts::load(&root)?;

    match command {
        "verify" => {
            contracts.verify()?;
            println!("contract verify passed");
        }
        "codegen" => {
            contracts.verify()?;
            run_legacy_generator(&root, false)?;
        }
        "check" => {
            contracts.verify()?;
            run_legacy_generator(&root, true)?;
        }
        "help" | "-h" | "--help" => print_help(),
        other => {
            print_help();
            bail!("unknown xtask command: {other}");
        }
    }

    Ok(())
}

fn print_help() {
    eprintln!("Usage: cargo xtask <verify|codegen|check>");
    eprintln!("  verify  Validate binding contract source files");
    eprintln!("  codegen  Verify and generate binding artifacts");
    eprintln!("  check   Verify and assert generated binding artifacts are fresh");
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .context("codegen crate must live under the flare-im-core-sdk workspace")
}

fn run_legacy_generator(root: &Path, check: bool) -> Result<()> {
    let script = root.join("bindings/contract/tools/generate_bindings.py");
    let mut command = Command::new("python3");
    command.arg(&script);
    if check {
        command.arg("--check");
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn {}", script.display()))?;
    if !status.success() {
        bail!("legacy binding generator exited with {status}");
    }
    Ok(())
}

#[derive(Debug)]
struct Contracts {
    manifest: Manifest,
    apis: Apis,
    events: Events,
    errors: Errors,
    dispatch: Dispatch,
    direct_invoke: DirectInvoke,
    c_typed_abi: CTypedAbi,
}

impl Contracts {
    fn load(root: &Path) -> Result<Self> {
        let dir = root.join("bindings/contract");
        Ok(Self {
            manifest: read_json(&dir, "manifest.json")?,
            apis: read_json(&dir, "apis.json")?,
            events: read_json(&dir, "events.json")?,
            errors: read_json(&dir, "errors.json")?,
            dispatch: read_json(&dir, "dispatch.json")?,
            direct_invoke: read_json(&dir, "direct_invoke.json")?,
            c_typed_abi: read_json(&dir, "c_typed_abi.json")?,
        })
    }

    fn verify(&self) -> Result<()> {
        require_non_empty("manifest.contractVersion", &self.manifest.contract_version)?;
        require_non_empty("apis.apiContractVersion", &self.apis.api_contract_version)?;
        require_non_empty(
            "events.eventContractVersion",
            &self.events.event_contract_version,
        )?;
        require_non_empty(
            "errors.errorContractVersion",
            &self.errors.error_contract_version,
        )?;

        let api_ids = self.api_ids();
        ensure_unique("apis.json method ids", api_ids.iter().map(String::as_str))?;
        ensure_no_removed_api_aliases(&api_ids)?;

        let event_ids = self
            .events
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>();
        ensure_unique("events.json event ids", event_ids)?;
        let event_codes = self
            .events
            .events
            .iter()
            .map(|event| event.c_code)
            .collect::<Vec<_>>();
        ensure_unique("events.json C event codes", event_codes)?;

        let error_codes = self.errors.c_abi.codes.as_slice();
        ensure_unique(
            "errors.json error names",
            error_codes.iter().map(|item| item.name.as_str()),
        )?;
        ensure_unique(
            "errors.json error codes",
            error_codes.iter().map(|item| item.code),
        )?;

        let mut dispatch_groups = BTreeMap::<String, BTreeSet<String>>::new();
        for group in &self.dispatch.groups {
            let mut names = Vec::new();
            for operation in &group.operations {
                if operation
                    .aliases
                    .as_ref()
                    .is_some_and(|aliases| !aliases.is_empty())
                {
                    bail!(
                        "dispatch.json group {} contains removed compatibility aliases on op {}",
                        group.id,
                        operation.op
                    );
                }
                names.push(operation.op.clone());
            }
            ensure_unique(
                &format!("dispatch.json group {} operation names", group.id),
                names.iter().map(String::as_str),
            )?;
            dispatch_groups.insert(group.id.clone(), names.into_iter().collect());
        }
        ensure_unique(
            "dispatch.json group ids",
            self.dispatch.groups.iter().map(|group| group.id.as_str()),
        )?;

        ensure_unique(
            "direct_invoke.json routes",
            self.direct_invoke
                .routes
                .iter()
                .map(|route| route.route.as_str()),
        )?;

        for method in self.apis.modules.iter().flat_map(|module| &module.methods) {
            for (symbol, dispatch_op) in c_api_entries(method.c.as_ref()) {
                let Some(dispatch_op) = dispatch_op else {
                    continue;
                };
                let group = c_symbol_runtime_group(&symbol);
                let Some(ops) = dispatch_groups.get(&group) else {
                    bail!(
                        "apis.json method {} references unknown C dispatch group {group:?} via {symbol:?}",
                        method.id
                    );
                };
                if !ops.contains(&dispatch_op) {
                    bail!(
                        "apis.json method {} references missing dispatch op {group}.{dispatch_op}",
                        method.id
                    );
                }
            }
        }

        for export in &self.c_typed_abi.exports {
            if let Some(api_id) = &export.api_id
                && !api_ids.contains(api_id)
            {
                bail!(
                    "c_typed_abi.json export {} references missing api_id {api_id}",
                    export.symbol
                );
            }
        }

        Ok(())
    }

    fn api_ids(&self) -> BTreeSet<String> {
        self.apis
            .modules
            .iter()
            .flat_map(|module| module.methods.iter().map(|method| method.id.clone()))
            .collect()
    }
}

fn read_json<T>(dir: &Path, name: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let path = dir.join(name);
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&data).with_context(|| format!("failed to parse {}", path.display()))
}

fn require_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn ensure_unique<T, I>(label: &str, values: I) -> Result<()>
where
    T: ToString,
    I: IntoIterator<Item = T>,
{
    let mut seen = BTreeSet::<String>::new();
    let mut duplicates = BTreeSet::<String>::new();
    for value in values {
        let value = value.to_string();
        if !seen.insert(value.clone()) {
            duplicates.insert(value);
        }
    }
    if !duplicates.is_empty() {
        let joined = duplicates
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        bail!("{label} contains duplicate values: {joined}");
    }
    Ok(())
}

fn ensure_no_removed_api_aliases(api_ids: &BTreeSet<String>) -> Result<()> {
    let removed_prefixes = ["events.", "messages.", "conversations.", "capabilities."];
    let removed_ids = ["media.get_file_url", "sync.set_conversation_input_state"];
    let offenders = api_ids
        .iter()
        .filter(|api_id| {
            removed_ids.contains(&api_id.as_str())
                || removed_prefixes
                    .iter()
                    .any(|prefix| api_id.starts_with(prefix))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !offenders.is_empty() {
        bail!(
            "apis.json contains removed compatibility API ids; use singular canonical contract ids instead: {}",
            offenders.join(", ")
        );
    }
    Ok(())
}

fn c_api_entries(value: Option<&Value>) -> Vec<(String, Option<String>)> {
    let Some(value) = value else {
        return Vec::new();
    };
    let values = match value {
        Value::Array(items) => items.iter().collect::<Vec<_>>(),
        other => vec![other],
    };

    values
        .into_iter()
        .filter_map(Value::as_str)
        .filter_map(|item| {
            let (symbol, dispatch) = item
                .split_once(':')
                .map_or((item, None), |(symbol, dispatch)| (symbol, Some(dispatch)));
            (!symbol.is_empty()).then(|| (symbol.to_string(), dispatch.map(str::to_string)))
        })
        .collect()
}

fn c_symbol_runtime_group(symbol: &str) -> String {
    let channel = symbol
        .strip_prefix("flare_")
        .unwrap_or(symbol)
        .strip_suffix("_json")
        .unwrap_or(symbol);
    if channel == "message_build" {
        return "message_build".to_string();
    }
    channel.replace("_dispatch", "")
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(rename = "contractVersion")]
    contract_version: String,
}

#[derive(Debug, Deserialize)]
struct Apis {
    #[serde(rename = "apiContractVersion")]
    api_contract_version: String,
    modules: Vec<ApiModule>,
}

#[derive(Debug, Deserialize)]
struct ApiModule {
    #[allow(dead_code)]
    id: String,
    methods: Vec<ApiMethod>,
}

#[derive(Debug, Deserialize)]
struct ApiMethod {
    id: String,
    #[serde(default)]
    c: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Events {
    #[serde(rename = "eventContractVersion")]
    event_contract_version: String,
    events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
struct Event {
    id: String,
    #[serde(rename = "cCode")]
    c_code: i32,
}

#[derive(Debug, Deserialize)]
struct Errors {
    #[serde(rename = "errorContractVersion")]
    error_contract_version: String,
    #[serde(rename = "cAbi")]
    c_abi: CAbiErrors,
}

#[derive(Debug, Deserialize)]
struct CAbiErrors {
    codes: Vec<CAbiErrorCode>,
}

#[derive(Debug, Deserialize)]
struct CAbiErrorCode {
    name: String,
    code: i32,
}

#[derive(Debug, Deserialize)]
struct Dispatch {
    groups: Vec<DispatchGroup>,
}

#[derive(Debug, Deserialize)]
struct DispatchGroup {
    id: String,
    operations: Vec<DispatchOperation>,
}

#[derive(Debug, Deserialize)]
struct DispatchOperation {
    op: String,
    #[serde(default)]
    aliases: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DirectInvoke {
    routes: Vec<DirectInvokeRoute>,
}

#[derive(Debug, Deserialize)]
struct DirectInvokeRoute {
    route: String,
}

#[derive(Debug, Deserialize)]
struct CTypedAbi {
    exports: Vec<CTypedAbiExport>,
}

#[derive(Debug, Deserialize)]
struct CTypedAbiExport {
    symbol: String,
    #[serde(default)]
    api_id: Option<String>,
}
