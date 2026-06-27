use anyhow::{Context, Result, bail};
use regex::Regex;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn load_json(path: &Path) -> Result<Value> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("invalid json: {}", path.display()))
}

pub(crate) fn files_under(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    collect_files(path, &mut out)?;
    out.sort();
    Ok(out)
}

pub(crate) fn files_with_extension(path: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    Ok(files_under(path)?
        .into_iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .collect())
}

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry?;
        let child = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(&child, out)?;
        } else if file_type.is_file() {
            out.push(child);
        }
    }
    Ok(())
}

pub(crate) fn find_matching<F>(
    root: &Path,
    rel_roots: &[&str],
    predicate: F,
) -> Result<Option<PathBuf>>
where
    F: Fn(&Path) -> bool,
{
    for rel_root in rel_roots {
        for path in files_under(&root.join(rel_root))? {
            if predicate(&path) {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

pub(crate) fn file_contains(path: &Path, regex: &Regex) -> Result<bool> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(regex.is_match(&text)),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Ok(false),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub(crate) fn upsert_text_file(
    path: &Path,
    content: &str,
    check: bool,
    drifted: &mut Vec<String>,
) -> Result<()> {
    upsert_bytes_file(path, content.as_bytes(), check, drifted)
}

pub(crate) fn upsert_bytes_file(
    path: &Path,
    content: &[u8],
    check: bool,
    drifted: &mut Vec<String>,
) -> Result<()> {
    if fs::read(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if check {
        drifted.push(path.display().to_string());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn remove_output_paths<I>(paths: I) -> Result<()>
where
    I: IntoIterator<Item = PathBuf>,
{
    for path in paths {
        if path.is_dir() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        } else if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn run_command(cwd: &Path, command: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(command)
        .current_dir(cwd)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {command}"))?;
    if !status.success() {
        bail!("{command} {} exited with {status}", args.join(" "));
    }
    Ok(())
}

pub(crate) fn run_optional_command(
    cwd: &Path,
    command: &str,
    args: &[&str],
    missing_message: &str,
) -> Result<()> {
    let available = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        println!("{missing_message}");
        return Ok(());
    }
    run_command(cwd, command, args)
}
