//! SQLite 运行时：持有连接池，供应用/扩展统一使用。

use std::sync::Arc;

use anyhow::Result as AnyhowResult;
use sqlx::SqlitePool;

use crate::create_pool;
use crate::schema_registry;

/// SQLite 存储运行时：持有连接池，创建后执行所有已注册的 schema 初始化。
#[derive(Clone)]
pub struct SqliteRuntime {
    pool: SqlitePool,
}

impl SqliteRuntime {
    /// 创建连接池并执行所有 [register_schema_init] 的初始化器，返回运行时。
    pub async fn open(database_url: &str) -> AnyhowResult<Arc<Self>> {
        let pool = create_pool(database_url).await?;
        schema_registry::run_registered_schema_inits(&pool).await?;
        Ok(Arc::new(Self { pool }))
    }

    /// 获取连接池，供扩展或 core-sdk 建表 / 构建 Repo。
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
