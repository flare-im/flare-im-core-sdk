//! 数据目录、`dataUrl` 解析、按用户落库路径等 **无状态工具**（不依赖 `IMClient`）。

use std::path::PathBuf;

use crate::error::ErrorCode;
use crate::FlareError;
use crate::Result;

/// 将 `user_id` 转为可安全用作目录名的片段。
pub fn sanitize_user_id_for_dir(user_id: &str) -> String {
    let s = user_id
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ if c.is_control() => '_',
            _ => c,
        })
        .collect::<String>();
    if s.is_empty() {
        "default".to_string()
    } else {
        s
    }
}

/// 开发时相对工作目录的数据根（`temp-data`），供示例 / Tauri dev 使用。
pub fn dev_data_dir_relative_to_cwd() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let base = if cwd
        .file_name()
        .is_some_and(|n| n == std::ffi::OsStr::new("src-tauri"))
    {
        cwd.join("..").join("..")
    } else {
        cwd.join("..")
    };
    let temp_data = base.join("temp-data");
    let _ = std::fs::create_dir_all(&temp_data);
    temp_data
}

/// 将 `init` 传入的 `dataUrl`（如 `file:///path`）解析为本地路径。
pub fn parse_data_url_to_path(data_url: &str) -> Result<PathBuf> {
    let t = data_url.trim();
    if t.is_empty() {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "data_url is empty",
        ));
    }
    if let Some(rest) = t.strip_prefix("file://") {
        let rest = rest.trim();
        #[cfg(windows)]
        {
            let s = rest.trim_start_matches('/');
            if s.len() >= 2 && s.as_bytes().get(1) == Some(&b':') {
                return Ok(PathBuf::from(s));
            }
        }
        return Ok(PathBuf::from(rest));
    }
    Ok(PathBuf::from(t))
}

/// `data_root` 下按用户隔离的 SQLite 文件路径（`{sanitized_user_id}/flare_im_sdk.db`）。
pub fn resolve_user_db_path(base: &std::path::Path, user_id: &str) -> PathBuf {
    let user_dir = sanitize_user_id_for_dir(user_id);
    let user_data_dir = base.join(user_dir);
    let _ = std::fs::create_dir_all(&user_data_dir);
    user_data_dir.join("flare_im_sdk.db")
}
