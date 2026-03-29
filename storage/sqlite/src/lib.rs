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
use sqlx::SqlitePool;
use std::path::Path;

pub use runtime::SqliteRuntime;
pub use schema_registry::{SchemaInitializer, register_schema_init, register_schema_init_with};

/// 创建 SQLite 连接池（不建表）
pub async fn create_pool(database_url: &str) -> AnyhowResult<SqlitePool> {
    Ok(SqlitePool::connect(database_url).await?)
}

/// 将本地文件路径转为 `sqlite://` URL
pub fn database_url_from_path(path: &Path) -> String {
    let path_str = path.to_string_lossy().replace('\\', "/");
    if path_str.starts_with('/') {
        format!("sqlite://{}?mode=rwc", path_str)
    } else {
        format!("sqlite:///{}?mode=rwc", path_str)
    }
}

/// 创建连接池并执行所有已注册的 [register_schema_init] 初始化器。
pub async fn open_pool(database_url: &str) -> AnyhowResult<SqlitePool> {
    let pool = create_pool(database_url).await?;
    schema_registry::run_registered_schema_inits(&pool).await?;
    Ok(pool)
}
