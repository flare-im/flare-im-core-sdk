//! SQLite 连接与运行时（不依赖 flare-im-core-sdk）
//!
//! 提供：
//! - [create_pool]：创建 SQLite 连接池
//! - [open_pool]：创建池并执行所有已注册 [register_schema_init] 的初始化器
//! - **[SqliteRuntime]**：运行时持有 pool，创建后执行已注册的 schema init
//! - [register_schema_init]：注册自定义 schema，在创建 pool 后统一调用
//!
//! Core 表与仓储由调用方（如 flare-im-core-sdk）在拿到 pool 后自行 init 与构建。

mod runtime;
mod schema_registry;

use anyhow::Result as AnyhowResult;
use log::LevelFilter;
use sqlx::ConnectOptions;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

pub use runtime::SqliteRuntime;
pub use schema_registry::{SchemaInitializer, register_schema_init, register_schema_init_with};

/// 统一连接选项：WAL + 较长 busy_timeout + 页缓存，减轻多连接争用写锁时的阻塞与 sqlx 慢查询告警。
pub(crate) fn connect_options(database_url: &str) -> AnyhowResult<SqliteConnectOptions> {
    Ok(SqliteConnectOptions::from_str(database_url)
        .map_err(|e| anyhow::anyhow!("invalid sqlite url: {e}"))?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        // 写锁排队：过短会导致语句失败或总耗时仍被记为 slow；15s 与池 acquire 超时配合
        .busy_timeout(Duration::from_secs(15))
        // 负值表示 KB，约 64MiB page cache，降低大库随机读延迟
        .pragma("cache_size", "-65536")
        // 适度 mmap，减少读系统调用（移动端/桌面均可接受）
        .pragma("mmap_size", "67108864")
        // 仅当单语句（含等锁）≥ 5s 再打 WARN，避免正常锁等待刷屏
        .log_slow_statements(LevelFilter::Warn, Duration::from_secs(5)))
}

/// 创建 SQLite 连接池（不建表）
///
/// 默认 `max_connections = 20`：单库写仍串行，但可减少「池耗尽 → 长时间等连接」导致的
/// `sqlx::pool::acquire` 与 `PRAGMA foreign_keys` 等初始化被拖慢。
pub async fn create_pool(database_url: &str) -> AnyhowResult<SqlitePool> {
    let max = std::env::var("FLARE_SQLITE_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| (1..=256).contains(n))
        .unwrap_or(20);

    Ok(SqlitePoolOptions::new()
        .max_connections(max)
        // 新连接建立 + busy 等锁可能 >2s，提高阈值减少误报
        .acquire_slow_threshold(Duration::from_secs(8))
        .connect_with(connect_options(database_url)?)
        .await?)
}

/// 将本地文件路径转为 `sqlite://` URL
pub fn database_url_from_path(path: &Path) -> String {
    if let Ok(file_url) = url::Url::from_file_path(path) {
        return format!("sqlite://{}?mode=rwc", file_url.path());
    }

    let path_str = path.to_string_lossy().replace('\\', "/");
    let encoded = path_str.replace(' ', "%20");
    if encoded.starts_with('/') {
        format!("sqlite://{}?mode=rwc", encoded)
    } else {
        format!("sqlite:///{}?mode=rwc", encoded)
    }
}

/// 创建连接池并执行所有已注册的 [register_schema_init] 初始化器。
pub async fn open_pool(database_url: &str) -> AnyhowResult<SqlitePool> {
    let pool = create_pool(database_url).await?;
    schema_registry::run_registered_schema_inits(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_from_path_encodes_spaces() {
        let url = database_url_from_path(Path::new("/tmp/Application Support/flare_im_sdk.db"));

        assert!(url.starts_with("sqlite:///tmp/"));
        assert!(url.contains("Application%20Support"));
        assert!(url.ends_with("?mode=rwc"));
    }

    #[tokio::test]
    async fn create_pool_supports_paths_with_spaces() {
        let root =
            std::env::temp_dir().join(format!("flare sqlite path test {}", std::process::id()));
        let db = root.join("flare im sdk.db");
        std::fs::create_dir_all(&root).expect("create temp sqlite dir");

        let url = database_url_from_path(&db);
        let pool = create_pool(&url).await.expect("open sqlite db");
        pool.close().await;

        assert!(db.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
