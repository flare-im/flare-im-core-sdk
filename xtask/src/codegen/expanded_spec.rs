use anyhow::{Result, bail};
use std::path::Path;

use crate::{load_expanded_client_spec, upsert_text_file};

pub(crate) fn emit_expanded_client_spec_file(root: &Path, check: bool) -> Result<()> {
    let spec = load_expanded_client_spec(root)?;
    let body = format!("{}\n", serde_json::to_string_pretty(&spec)?);
    let mut drifted = Vec::new();
    upsert_text_file(
        &root.join("sdk-spec/generated/client_spec.json"),
        &body,
        check,
        &mut drifted,
    )?;
    if !drifted.is_empty() {
        let details = drifted.join("\n  - ");
        bail!("Rust-owned expanded sdk-spec snapshot drifted:\n  - {details}");
    }
    if !check {
        println!("Rust-owned expanded SDK spec generated");
    }
    Ok(())
}
