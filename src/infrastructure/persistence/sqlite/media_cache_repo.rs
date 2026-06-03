use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use tokio::sync::RwLock;

use crate::domain::{MediaCacheAdmin, MediaCacheEntryVo, MediaCacheStatsVo, MediaCacheStore};
use crate::shared::error::{ErrorCode, FlareError, Result};

#[derive(Debug, Clone)]
struct MediaCacheState {
    max_bytes: u64,
    root_override: Option<PathBuf>,
}

pub struct SqliteMediaCacheRepo {
    pool: SqlitePool,
    /// 与 `flare_im_sdk.db` 同目录下的 `media_cache`（未自定义目录时生效）
    default_root: PathBuf,
    state: Arc<RwLock<MediaCacheState>>,
}

impl SqliteMediaCacheRepo {
    /// 从库中读取 `media_cache_settings` 并完成初始化。
    pub async fn create(pool: SqlitePool, default_root: PathBuf) -> Result<Self> {
        let _ = tokio::fs::create_dir_all(&default_root).await;

        let row = sqlx::query(
            "SELECT max_bytes, cache_root FROM media_cache_settings WHERE singleton = 1",
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        let (max_bytes, root_override) = if let Some(row) = row {
            let mb: i64 = row.get("max_bytes");
            let root_s: String = row.get("cache_root");
            let t = root_s.trim();
            let ov = if t.is_empty() {
                None
            } else {
                Some(PathBuf::from(t))
            };
            (mb.max(0) as u64, ov)
        } else {
            (0u64, None)
        };

        Ok(Self {
            pool,
            default_root,
            state: Arc::new(RwLock::new(MediaCacheState {
                max_bytes,
                root_override,
            })),
        })
    }

    async fn effective_root(&self) -> PathBuf {
        let s = self.state.read().await;
        s.root_override
            .clone()
            .unwrap_or_else(|| self.default_root.clone())
    }

    /// 按 MIME 主类型分目录：图片 / 音频 / 视频 / 其余（对应 `image`、`audio`、`video`、`other`）。
    fn media_kind_dir(mime_type: &str) -> &'static str {
        fn starts_with_icase(hay: &str, needle: &str) -> bool {
            hay.len() >= needle.len() && hay[..needle.len()].eq_ignore_ascii_case(needle)
        }
        let t = mime_type.trim();
        if starts_with_icase(t, "image/") {
            "image"
        } else if starts_with_icase(t, "audio/") {
            "audio"
        } else if starts_with_icase(t, "video/") {
            "video"
        } else {
            "other"
        }
    }

    fn blob_path(root: &Path, file_id: &str, mime_type: &str) -> PathBuf {
        let kind = Self::media_kind_dir(mime_type);
        let mut hasher = Sha256::new();
        hasher.update(file_id.as_bytes());
        let hex = format!("{:x}", hasher.finalize());
        let dir = &hex[..2];
        let name = &hex[2..];
        root.join(kind).join(dir).join(name)
    }

    async fn trim_to_max_bytes(&self) -> Result<()> {
        let max = { self.state.read().await.max_bytes };
        if max == 0 {
            return Ok(());
        }
        let max_i64 = i64::try_from(max).unwrap_or(i64::MAX);
        loop {
            let sum: i64 =
                sqlx::query_scalar("SELECT COALESCE(SUM(size_bytes), 0) FROM media_local_cache")
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
            if sum <= max_i64 {
                break;
            }
            if !self.remove_oldest_entry().await? {
                break;
            }
        }
        Ok(())
    }

    async fn remove_oldest_entry(&self) -> Result<bool> {
        let row =
            sqlx::query("SELECT file_id FROM media_local_cache ORDER BY updated_at_ms ASC LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let Some(row) = row else {
            return Ok(false);
        };
        let fid: String = row.get("file_id");
        self.remove(&fid).await?;
        Ok(true)
    }

    async fn clear_all_entries(&self) -> Result<()> {
        let rows = sqlx::query("SELECT local_path FROM media_local_cache")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        sqlx::query("DELETE FROM media_local_cache")
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        for row in rows {
            let p: String = row.get("local_path");
            let _ = tokio::fs::remove_file(Path::new(&p)).await;
        }
        Ok(())
    }
}

#[async_trait]
impl MediaCacheStore for SqliteMediaCacheRepo {
    async fn get_cached(&self, file_id: &str) -> Result<Option<MediaCacheEntryVo>> {
        let fid = file_id.trim();
        if fid.is_empty() {
            return Ok(None);
        }
        let row = sqlx::query(
            r#"SELECT file_id, local_path, mime_type, size_bytes, updated_at_ms
               FROM media_local_cache WHERE file_id = ?"#,
        )
        .bind(fid)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let path: String = row.get("local_path");
        if tokio::fs::metadata(&path).await.is_err() {
            let _ = sqlx::query("DELETE FROM media_local_cache WHERE file_id = ?")
                .bind(fid)
                .execute(&self.pool)
                .await;
            return Ok(None);
        }

        Ok(Some(MediaCacheEntryVo {
            file_id: row.get("file_id"),
            local_path: path,
            mime_type: row.get("mime_type"),
            size_bytes: row.get("size_bytes"),
            updated_at_ms: row.get("updated_at_ms"),
        }))
    }

    async fn put_bytes(
        &self,
        file_id: &str,
        data: &[u8],
        mime_type: &str,
    ) -> Result<MediaCacheEntryVo> {
        let fid = file_id.trim();
        if fid.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "media cache: empty file_id",
            ));
        }

        let root = self.effective_root().await;
        let _ = tokio::fs::create_dir_all(&root).await;

        let dest = Self::blob_path(&root, fid, mime_type);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| FlareError::system(format!("media cache mkdir: {e}")))?;
        }

        let tmp = dest.with_extension("part");
        tokio::fs::write(&tmp, data)
            .await
            .map_err(|e| FlareError::system(format!("media cache write: {e}")))?;
        tokio::fs::rename(&tmp, &dest)
            .await
            .map_err(|e| FlareError::system(format!("media cache rename: {e}")))?;

        let local_path = dest.to_string_lossy().to_string();
        let size_bytes = i64::try_from(data.len())
            .map_err(|_| FlareError::general_error("media cache: payload too large"))?;
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"INSERT INTO media_local_cache (file_id, local_path, mime_type, size_bytes, updated_at_ms)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(file_id) DO UPDATE SET
                 local_path = excluded.local_path,
                 mime_type = excluded.mime_type,
                 size_bytes = excluded.size_bytes,
                 updated_at_ms = excluded.updated_at_ms"#,
        )
        .bind(fid)
        .bind(&local_path)
        .bind(mime_type)
        .bind(size_bytes)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        self.trim_to_max_bytes().await?;

        Ok(MediaCacheEntryVo {
            file_id: fid.to_string(),
            local_path,
            mime_type: mime_type.to_string(),
            size_bytes,
            updated_at_ms: now,
        })
    }

    async fn remove(&self, file_id: &str) -> Result<()> {
        let fid = file_id.trim();
        if fid.is_empty() {
            return Ok(());
        }
        let row = sqlx::query("SELECT local_path FROM media_local_cache WHERE file_id = ?")
            .bind(fid)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        if let Some(row) = row {
            let path: String = row.get("local_path");
            let _ = tokio::fs::remove_file(Path::new(&path)).await;
        }
        sqlx::query("DELETE FROM media_local_cache WHERE file_id = ?")
            .bind(fid)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl MediaCacheAdmin for SqliteMediaCacheRepo {
    async fn media_cache_stats(&self) -> Result<MediaCacheStatsVo> {
        let effective = self.effective_root().await;
        let default_s = self.default_root.to_string_lossy().to_string();
        let max_bytes = { self.state.read().await.max_bytes };
        let total: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(size_bytes), 0) FROM media_local_cache")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        let entry_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_local_cache")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(MediaCacheStatsVo {
            effective_root: effective.to_string_lossy().to_string(),
            default_root: default_s,
            max_bytes,
            total_bytes: total,
            entry_count,
        })
    }

    async fn set_media_cache_max_bytes(&self, max_bytes: u64) -> Result<()> {
        let mb_i64 = i64::try_from(max_bytes).unwrap_or(i64::MAX);
        sqlx::query("UPDATE media_cache_settings SET max_bytes = ? WHERE singleton = 1")
            .bind(mb_i64)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        self.state.write().await.max_bytes = max_bytes;
        self.trim_to_max_bytes().await
    }

    async fn set_media_cache_root(&self, absolute_path: Option<&str>) -> Result<()> {
        self.clear_all_entries().await?;

        let stored = match absolute_path.map(str::trim).filter(|s| !s.is_empty()) {
            None => String::new(),
            Some(p) => {
                let pb = PathBuf::from(p);
                if !pb.is_absolute() {
                    return Err(FlareError::localized(
                        ErrorCode::InvalidParameter,
                        "media cache root must be an absolute path",
                    ));
                }
                let _ = tokio::fs::create_dir_all(&pb)
                    .await
                    .map_err(|e| FlareError::system(format!("media cache root not usable: {e}")))?;
                p.to_string()
            }
        };

        sqlx::query("UPDATE media_cache_settings SET cache_root = ? WHERE singleton = 1")
            .bind(&stored)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;

        let mut w = self.state.write().await;
        w.root_override = if stored.is_empty() {
            None
        } else {
            Some(PathBuf::from(stored))
        };
        Ok(())
    }

    async fn clear_media_cache(&self) -> Result<()> {
        self.clear_all_entries().await
    }
}
