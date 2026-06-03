//! SQLite：`user_file_download` + `file_download_settings`

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::domain::UserFileDownloadStore;
use crate::shared::error::{ErrorCode, FlareError, Result};

const DEFAULT_SUBFOLDER: &str = "flare";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub struct SqliteUserFileDownloadRepo {
    pool: SqlitePool,
}

impl SqliteUserFileDownloadRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn sanitize_subfolder(name: &str) -> String {
    let t = name.trim();
    if t.is_empty() {
        return DEFAULT_SUBFOLDER.to_string();
    }
    let cleaned: String = t
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '_') {
        DEFAULT_SUBFOLDER.to_string()
    } else {
        cleaned
    }
}

#[async_trait]
impl UserFileDownloadStore for SqliteUserFileDownloadRepo {
    async fn get_saved_path(&self, download_key: &str) -> Result<Option<String>> {
        let k = download_key.trim();
        if k.is_empty() {
            return Ok(None);
        }
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT local_path FROM user_file_download WHERE download_key = ? LIMIT 1",
        )
        .bind(k)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row.map(|r| r.0))
    }

    async fn save_download_record(
        &self,
        download_key: &str,
        local_path: &str,
        display_name: &str,
    ) -> Result<()> {
        let k = download_key.trim();
        if k.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "empty download_key",
            ));
        }
        let p = local_path.trim();
        if p.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "empty local_path",
            ));
        }
        let t = now_ms();
        sqlx::query(
            r#"INSERT INTO user_file_download (download_key, local_path, display_name, updated_at_ms)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(download_key) DO UPDATE SET
                 local_path = excluded.local_path,
                 display_name = excluded.display_name,
                 updated_at_ms = excluded.updated_at_ms"#,
        )
        .bind(k)
        .bind(p)
        .bind(display_name)
        .bind(t)
        .execute(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn get_download_subfolder(&self) -> Result<String> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT download_subfolder FROM file_download_settings WHERE singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row
            .map(|r| sanitize_subfolder(&r.0))
            .unwrap_or_else(|| DEFAULT_SUBFOLDER.to_string()))
    }

    async fn set_download_subfolder(&self, name: &str) -> Result<()> {
        let s = if name.trim().is_empty() {
            DEFAULT_SUBFOLDER.to_string()
        } else {
            sanitize_subfolder(name)
        };
        sqlx::query("UPDATE file_download_settings SET download_subfolder = ? WHERE singleton = 1")
            .bind(&s)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }

    async fn delete_download_record(&self, download_key: &str) -> Result<()> {
        let k = download_key.trim();
        if k.is_empty() {
            return Ok(());
        }
        sqlx::query("DELETE FROM user_file_download WHERE download_key = ?")
            .bind(k)
            .execute(&self.pool)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}
