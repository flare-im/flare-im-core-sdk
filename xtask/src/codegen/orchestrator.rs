use anyhow::{Result, bail};
use std::{collections::BTreeSet, path::Path};

use super::{
    emit_bridge_files, emit_doc_files, emit_expanded_client_spec_file, emit_platform_adapter_files,
    emit_platform_api_files, emit_platform_contract_files, emit_typescript_adapter_files,
    emit_typescript_contract_files, emit_wire_boundaries, snapshot::generated_output_snapshot,
};

pub(crate) fn run_codegen(root: &Path, check: bool) -> Result<()> {
    let before = if check {
        Some(generated_output_snapshot(root)?)
    } else {
        None
    };
    emit_expanded_client_spec_file(root, false)?;
    emit_doc_files(root, false)?;
    emit_platform_contract_files(root, false)?;
    emit_platform_api_files(root, false)?;
    emit_platform_adapter_files(root, false)?;
    emit_typescript_contract_files(root, false)?;
    emit_typescript_adapter_files(root, false)?;
    emit_bridge_files(root, false)?;
    emit_wire_boundaries(root, false)?;
    if let Some(before) = before {
        let after = generated_output_snapshot(root)?;
        if before != after {
            let mut changed = BTreeSet::new();
            for key in before.keys().chain(after.keys()) {
                if before.get(key) != after.get(key) {
                    changed.insert(key.display().to_string());
                }
            }
            let details = changed
                .into_iter()
                .take(40)
                .collect::<Vec<_>>()
                .join("\n  - ");
            bail!(
                "generated outputs drifted; run `make codegen` and review the results:\n  - {details}"
            );
        }
    }
    Ok(())
}
