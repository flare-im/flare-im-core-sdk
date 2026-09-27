//! 原生端「保存到本机」与媒体缓存填充的底层步骤：平台默认下载目录、目录可写校验、
//! 流式落盘到临时文件（完成才改名、失败必清理）、从网络填充本地缓存。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::application::callbacks::{FileDownloadProgress, FileDownloadProgressCallback};
use crate::domain::{MediaCacheEntryVo, MediaCacheStore};
use crate::infrastructure::transport::HttpClient;
use crate::shared::error::{ErrorCode, FlareError, Result};

/// 显示时自动缓存的单个文件上限：图片与缩略图都远小于它，超过的（大视频）只在显式保存时落盘。
pub(super) const AUTO_CACHE_MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;

/// 平台约定的「下载」根目录（不含子文件夹）。
///
/// - macOS / Windows / Linux：系统「下载」文件夹。
/// - Android：共享存储的 `Download/`（Android 11 起应用可直接在此创建文件，文件管理器与相册可见）。
/// - iOS：应用沙盒的 `Documents/`（开启文件共享后在「文件」App 里可见；沙盒里的 `Downloads/` 用户看不到）。
pub(super) fn platform_default_download_root() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let base = std::env::var_os("EXTERNAL_STORAGE")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from("/storage/emulated/0"));
        return Some(base.join("Download"));
    }
    #[cfg(target_os = "ios")]
    {
        return dirs::document_dir();
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        dirs::download_dir().or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
    }
}

/// 创建目录并实际写一次探测文件：目录存在不代表可写（Android 共享存储、只读卷、沙盒外路径）。
pub(super) async fn ensure_writable_dir(dir: &Path) -> Result<()> {
    if !dir.is_absolute() {
        return Err(FlareError::localized(
            ErrorCode::InvalidParameter,
            "download directory must be an absolute path",
        ));
    }
    tokio::fs::create_dir_all(dir).await.map_err(|e| {
        FlareError::localized(
            ErrorCode::PermissionDenied,
            format!("download directory is not usable: {e}"),
        )
    })?;
    let probe = dir.join(format!(
        ".flare-write-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::write(&probe, b"").await.map_err(|e| {
        FlareError::localized(
            ErrorCode::PermissionDenied,
            format!("download directory is not writable: {e}"),
        )
    })?;
    let _ = tokio::fs::remove_file(&probe).await;
    Ok(())
}

/// 常见媒体类型的扩展名：保存时名字没有扩展名（图片、视频通常没有文件名）就按类型补上，
/// 否则存下来的文件系统打不开。
pub(super) fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    Some(match essence.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/aac" => "aac",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        "application/pdf" => "pdf",
        "application/zip" => "zip",
        "text/plain" => "txt",
        _ => return None,
    })
}

/// 目标文件同目录下的临时文件名：隐藏、带专用后缀，完成后改名成目标。
pub(super) fn temp_sibling(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_string());
    dest.with_file_name(format!(".{name}.flaredownload"))
}

/// 一次落盘的结果。
pub(super) struct WrittenFile {
    pub size: u64,
    /// 响应头里的 `Content-Type`（去掉参数）；本地复制时为 `None`。
    pub content_type: Option<String>,
}

fn emit_progress(on: Option<&FileDownloadProgressCallback>, downloaded: u64, total: Option<u64>) {
    if let Some(cb) = on {
        cb(FileDownloadProgress { downloaded, total });
    }
}

pub(super) fn check_budget(
    total: Option<u64>,
    downloaded: u64,
    incoming: u64,
    max_bytes: u64,
) -> Result<()> {
    if total.is_some_and(|t| t > max_bytes) || downloaded.saturating_add(incoming) > max_bytes {
        return Err(FlareError::localized(
            ErrorCode::ResourceExhausted,
            format!("download exceeds {max_bytes} bytes"),
        ));
    }
    Ok(())
}

fn cancelled_error() -> FlareError {
    FlareError::localized(ErrorCode::OperationFailed, "download cancelled")
}

/// 把 `url` 流式写到 `dest`（通常是 [`temp_sibling`]）。任何失败或取消都删掉 `dest`。
pub(super) async fn stream_http_to_file(
    http: &HttpClient,
    url: &str,
    dest: &Path,
    max_bytes: u64,
    run_flag: Option<&AtomicBool>,
    on_progress: Option<&FileDownloadProgressCallback>,
) -> Result<WrittenFile> {
    let result = async {
        let resp = http.get_response_direct_url(url).await?;
        let total = resp.content_length();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .filter(|v| !v.is_empty());
        check_budget(total, 0, 0, max_bytes)?;
        emit_progress(on_progress, 0, total);
        let mut stream = resp.bytes_stream();
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| FlareError::general_error(format!("create dest: {e}")))?;
        let mut downloaded: u64 = 0;
        while let Some(item) = stream.next().await {
            if run_flag.is_some_and(|f| !f.load(Ordering::Relaxed)) {
                return Err(cancelled_error());
            }
            let chunk = item.map_err(|e| FlareError::system(format!("http chunk: {e}")))?;
            check_budget(total, downloaded, chunk.len() as u64, max_bytes)?;
            file.write_all(&chunk)
                .await
                .map_err(|e| FlareError::general_error(format!("write: {e}")))?;
            downloaded += chunk.len() as u64;
            emit_progress(on_progress, downloaded, total);
        }
        file.flush()
            .await
            .map_err(|e| FlareError::general_error(format!("flush: {e}")))?;
        if total.is_some_and(|t| t != downloaded) {
            return Err(FlareError::system(format!(
                "download truncated: {downloaded} of {} bytes",
                total.unwrap_or_default()
            )));
        }
        Ok(WrittenFile {
            size: downloaded,
            content_type,
        })
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

/// 本地文件复制到 `dest`（带进度与取消）。任何失败或取消都删掉 `dest`。
pub(super) async fn copy_file_to(
    src: &Path,
    dest: &Path,
    max_bytes: u64,
    run_flag: Option<&AtomicBool>,
    on_progress: Option<&FileDownloadProgressCallback>,
) -> Result<WrittenFile> {
    let result = async {
        if !src.is_file() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "source file does not exist",
            ));
        }
        let total = tokio::fs::metadata(src)
            .await
            .map_err(|e| FlareError::general_error(format!("metadata: {e}")))?
            .len();
        check_budget(Some(total), 0, 0, max_bytes)?;
        emit_progress(on_progress, 0, Some(total));
        let mut reader = tokio::fs::File::open(src)
            .await
            .map_err(|e| FlareError::general_error(format!("open source: {e}")))?;
        let mut writer = tokio::fs::File::create(dest)
            .await
            .map_err(|e| FlareError::general_error(format!("create dest: {e}")))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut copied: u64 = 0;
        loop {
            if run_flag.is_some_and(|f| !f.load(Ordering::Relaxed)) {
                return Err(cancelled_error());
            }
            let n = reader
                .read(&mut buf)
                .await
                .map_err(|e| FlareError::general_error(format!("read: {e}")))?;
            if n == 0 {
                break;
            }
            writer
                .write_all(&buf[..n])
                .await
                .map_err(|e| FlareError::general_error(format!("write: {e}")))?;
            copied += n as u64;
            emit_progress(on_progress, copied, Some(total));
        }
        writer
            .flush()
            .await
            .map_err(|e| FlareError::general_error(format!("flush: {e}")))?;
        Ok(WrittenFile {
            size: copied,
            content_type: None,
        })
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

/// 读文件头若干字节（用于识别 MIME）。
pub(super) async fn read_head(path: &Path, len: usize) -> Vec<u8> {
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    let mut buf = vec![0u8; len];
    let mut filled = 0;
    while filled < len {
        match file.read(&mut buf[filled..]).await {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
    }
    buf.truncate(filled);
    buf
}

/// 从网络把 `file_id` 填进本地缓存：流式写到缓存暂存目录，完成后整体登记。
///
/// `mime_of` 由调用方提供：响应头、URL 后缀、文件头依次推断。
pub(super) async fn fill_cache_from_url(
    http: &HttpClient,
    cache: &Arc<dyn MediaCacheStore>,
    file_id: &str,
    url: &str,
    max_bytes: u64,
    mime_of: impl FnOnce(&str, Option<&str>, &[u8]) -> String,
) -> Result<MediaCacheEntryVo> {
    let staged = cache
        .staging_dir()
        .await?
        .join(format!("{}.part", uuid::Uuid::new_v4().simple()));
    let written = stream_http_to_file(http, url, &staged, max_bytes, None, None).await?;
    let head = read_head(&staged, 16).await;
    let mime = mime_of(url, written.content_type.as_deref(), &head);
    let result = cache.put_file(file_id, &staged, &mime, true).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&staged).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_follows_the_media_type() {
        assert_eq!(extension_for_mime("image/png"), Some("png"));
        assert_eq!(
            extension_for_mime("IMAGE/JPEG; charset=binary"),
            Some("jpg")
        );
        assert_eq!(extension_for_mime("video/quicktime"), Some("mov"));
        assert_eq!(extension_for_mime("application/octet-stream"), None);
    }

    #[test]
    fn temp_sibling_is_hidden_next_to_target() {
        let dest = Path::new("/tmp/flare/report.pdf");
        assert_eq!(
            temp_sibling(dest),
            PathBuf::from("/tmp/flare/.report.pdf.flaredownload")
        );
    }

    #[test]
    fn budget_rejects_declared_and_streamed_overflow() {
        assert!(check_budget(Some(11), 0, 0, 10).is_err());
        assert!(check_budget(None, 8, 3, 10).is_err());
        assert!(check_budget(Some(10), 7, 3, 10).is_ok());
    }

    #[tokio::test]
    async fn writable_dir_is_created_and_probed() {
        let root = std::env::temp_dir().join(format!("flare-dl-{}", uuid::Uuid::new_v4().simple()));
        let dir = root.join("a").join("b");
        ensure_writable_dir(&dir).await.expect("writable");
        assert!(dir.is_dir());
        // 探测文件不留痕。
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(
            ensure_writable_dir(Path::new("relative/dir"))
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn copy_failure_leaves_no_partial_file() {
        let root = std::env::temp_dir().join(format!("flare-dl-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("src.bin");
        std::fs::write(&src, vec![7u8; 4096]).unwrap();
        let dest = root.join(".out.flaredownload");
        // 超出上限：必须失败且不留半截文件。
        assert!(copy_file_to(&src, &dest, 1024, None, None).await.is_err());
        assert!(!dest.exists());
        // 取消同理。
        let flag = AtomicBool::new(false);
        assert!(
            copy_file_to(&src, &dest, u64::MAX, Some(&flag), None)
                .await
                .is_err()
        );
        assert!(!dest.exists());
        let ok = copy_file_to(&src, &dest, u64::MAX, None, None)
            .await
            .unwrap();
        assert_eq!(ok.size, 4096);
        let _ = std::fs::remove_dir_all(root);
    }
}
