use std::fs;
use std::path::{Path, PathBuf};

fn rust_files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                if matches!(name, "target" | ".git") {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|v| v.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files
}

fn strip_cfg_test_modules(source: &str) -> String {
    let mut out = String::new();
    let mut pending_cfg_test = false;
    let mut in_test_module = false;
    let mut brace_depth: i32 = 0;

    for line in source.lines() {
        let trimmed = line.trim();
        if in_test_module {
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;
            if brace_depth <= 0 {
                in_test_module = false;
                brace_depth = 0;
            }
            continue;
        }

        if trimmed == "#[cfg(test)]" {
            pending_cfg_test = true;
            continue;
        }

        if pending_cfg_test && trimmed.starts_with("mod tests") {
            in_test_module = true;
            brace_depth += line.matches('{').count() as i32;
            brace_depth -= line.matches('}').count() as i32;
            if brace_depth <= 0 {
                brace_depth = 1;
            }
            pending_cfg_test = false;
            continue;
        }

        if !trimmed.starts_with("#[") {
            pending_cfg_test = false;
        }
        out.push_str(line);
        out.push('\n');
    }

    out
}

fn strip_line_comments(source: &str) -> String {
    let mut out = String::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            out.push('\n');
            continue;
        }
        let code = line.split_once("//").map(|(code, _)| code).unwrap_or(line);
        out.push_str(code);
        out.push('\n');
    }
    out
}

fn line_number_at(source: &str, byte_idx: usize) -> usize {
    source[..byte_idx].bytes().filter(|b| *b == b'\n').count() + 1
}

fn line_snippet(source: &str, line: usize) -> String {
    source
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn is_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn take_ident(input: &str) -> Option<&str> {
    let input = input.trim_start();
    let mut end = 0;
    for (idx, ch) in input.char_indices() {
        if idx == 0 && !(ch == '_' || ch.is_ascii_alphabetic()) {
            return None;
        }
        if !is_ident_char(ch) {
            break;
        }
        end = idx + ch.len_utf8();
    }
    (end > 0).then_some(&input[..end])
}

fn find_matching_brace(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_group_items(input: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(&input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    items.push(&input[start..]);
    items
}

fn core_sdk_roots_after_path(input: &str) -> Vec<String> {
    let input = input.trim_start();
    let Some(group) = input.strip_prefix('{') else {
        return vec![take_ident(input).unwrap_or("<crate-root>").to_string()];
    };

    let Some(end) = find_matching_brace(input) else {
        return vec!["<unterminated-group>".to_string()];
    };
    let group = &group[..end.saturating_sub(1)];
    split_top_level_group_items(group)
        .into_iter()
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() || item == "self" {
                return None;
            }
            Some(take_ident(item).unwrap_or("<crate-root>").to_string())
        })
        .collect()
}

#[derive(Debug)]
struct CoreSdkReference {
    line: usize,
    root: String,
    snippet: String,
}

fn collect_core_sdk_references(source: &str) -> Vec<CoreSdkReference> {
    const PREFIX: &str = "flare_im_core_sdk::";

    let mut refs = Vec::new();
    let mut search_from = 0;
    while let Some(relative_idx) = source[search_from..].find(PREFIX) {
        let start = search_from + relative_idx;
        let after_prefix = start + PREFIX.len();
        let line = line_number_at(source, start);
        let snippet = line_snippet(source, line);
        for root in core_sdk_roots_after_path(&source[after_prefix..]) {
            refs.push(CoreSdkReference {
                line,
                root,
                snippet: snippet.clone(),
            });
        }
        search_from = after_prefix;
    }
    refs
}

#[test]
fn core_source_does_not_depend_on_business_or_plugin_crates() {
    let core_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "flare_social",
        "flare-social",
        "flare_sdk_plugin",
        "flare-sdk-plugin",
    ];
    let mut violations = Vec::new();

    for file in rust_files_under(&core_src) {
        let source = fs::read_to_string(&file).expect("read core source");
        for needle in forbidden {
            if source.contains(needle) {
                violations.push(format!("{} contains {needle}", file.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "core must not depend on business/plugin crates:\n{}",
        violations.join("\n")
    );
}

#[test]
fn social_production_code_uses_spi_for_core_extension_boundaries() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let social_src = manifest
        .parent()
        .expect("flare-im root")
        .join("flare-social/flare-social-sdk/src");
    if !social_src.exists() {
        return;
    }

    let allowed_roots = ["spi", "model", "prelude"];
    let mut violations = Vec::new();

    for file in rust_files_under(&social_src) {
        let source = fs::read_to_string(&file).expect("read social source");
        let production_source = strip_line_comments(&strip_cfg_test_modules(&source));
        for reference in collect_core_sdk_references(&production_source) {
            if !allowed_roots.contains(&reference.root.as_str()) {
                violations.push(format!(
                    "{}:{} uses flare_im_core_sdk::{} outside the extension facade: {}",
                    file.display(),
                    reference.line,
                    reference.root,
                    reference.snippet
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "social production code may only use flare_im_core_sdk::{{spi, model, prelude}}:\n{}",
        violations.join("\n")
    );
}
