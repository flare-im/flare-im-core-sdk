use anyhow::{Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::{
    collections::BTreeMap,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};

use crate::files_under;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileDigest {
    len: u64,
    hash: u64,
}

pub(super) fn generated_output_snapshot(root: &Path) -> Result<BTreeMap<PathBuf, FileDigest>> {
    let mut snapshot = BTreeMap::new();
    for rel in GENERATED_OUTPUT_PATHS {
        let path = root.join(rel);
        if path.is_file() {
            snapshot.insert(PathBuf::from(rel), file_digest(&path)?);
        } else if path.is_dir() {
            for file in files_under(&path)? {
                if should_skip_snapshot_file(&file) {
                    continue;
                }
                let rel_path = file
                    .strip_prefix(root)
                    .with_context(|| format!("failed to relativize {}", file.display()))?
                    .to_path_buf();
                snapshot.insert(rel_path, file_digest(&file)?);
            }
        }
    }
    Ok(snapshot)
}

fn file_digest(path: &Path) -> Result<FileDigest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(FileDigest {
        len: bytes.len() as u64,
        hash: hasher.finish(),
    })
}

fn should_skip_snapshot_file(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            "node_modules" | ".dart_tool" | ".build" | "target" | ".DS_Store"
        )
    })
}

const GENERATED_OUTPUT_PATHS: &[&str] = &[
    "docs/client-api-reference.md",
    "docs/client-model-reference.md",
    "docs/events.md",
    "sdk-spec/GENERATED.md",
    "sdk-spec/generated/client_spec.json",
    "native/abi",
    // TS 包的 README 是手写的 npm 门面页，不归生成器管（见 docs.rs 里的说明）。
    "packages/flare-core-typescript-sdk/src",
    "packages/flare-core-android-sdk/README.md",
    "packages/flare-core-android-sdk/src",
    "packages/flare-core-apple-sdk/README.md",
    "packages/flare-core-apple-sdk/Sources",
    "packages/flare-core-flutter-sdk/README.md",
    "packages/flare-core-flutter-sdk/lib",
    "packages/flare-core-harmony-arkts-sdk/README.md",
    "packages/flare-core-harmony-arkts-sdk/src",
    "packages/flare-core-harmony-cangjie-sdk/README.md",
    "packages/flare-core-harmony-cangjie-sdk/src",
];
