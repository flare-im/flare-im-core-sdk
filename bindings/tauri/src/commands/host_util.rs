//! 宿主侧小工具命令（不经过 IMClient），供示例应用写临时文件等。

use base64::{Engine, engine::general_purpose::STANDARD};

/// 将 JPEG 的 base64（可含 `data:image/jpeg;base64,` 前缀）写入系统临时目录，返回绝对路径。
/// 用于视频首帧封面等场景；文件落在 `$TEMP/flare-im-preview/`。
#[tauri::command]
pub fn sdk_save_preview_jpeg_temp(base64_jpeg: String) -> Result<String, String> {
    let trimmed = base64_jpeg.trim();
    let b64 = trimmed
        .strip_prefix("data:image/jpeg;base64,")
        .or_else(|| trimmed.strip_prefix("data:image/jpg;base64,"))
        .unwrap_or(trimmed);
    let bytes = STANDARD.decode(b64).map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join("flare-im-preview");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("{}.jpg", uuid::Uuid::new_v4());
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}
