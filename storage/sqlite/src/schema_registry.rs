//! 自定义 schema 注册：在创建 pool 后统一执行 core init + 所有已注册的 init。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result as AnyhowResult;
use once_cell::sync::Lazy;
use sqlx::SqlitePool;

type SchemaInitEntry = (String, Arc<dyn SchemaInitializer>);
type SchemaInitRegistry = Mutex<Vec<SchemaInitEntry>>;
type SchemaInitSnapshot = Vec<SchemaInitEntry>;

/// 可注册的 schema 初始化器：接收 pool，执行异步初始化（如建表）。
pub trait SchemaInitializer: Send + Sync {
    fn run<'a>(
        &'a self,
        pool: &'a SqlitePool,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<()>> + Send + 'a>>;
}

/// 闭包形式（返回 anyhow::Result）
struct ClosureSchemaInit<F>(F);

impl<F, Fut> SchemaInitializer for ClosureSchemaInit<F>
where
    F: Fn(&SqlitePool) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = AnyhowResult<()>> + Send + 'static,
{
    fn run<'a>(
        &'a self,
        pool: &'a SqlitePool,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<()>> + Send + 'a>> {
        Box::pin((self.0)(pool))
    }
}

/// 闭包形式（返回 Result<(), E>，E 转为 anyhow::Error），便于直接传入 core-sdk 的 init_schema
struct MapErrSchemaInit<F>(F);

impl<F, Fut, E> SchemaInitializer for MapErrSchemaInit<F>
where
    F: Fn(&SqlitePool) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Into<anyhow::Error> + 'static,
{
    fn run<'a>(
        &'a self,
        pool: &'a SqlitePool,
    ) -> Pin<Box<dyn Future<Output = AnyhowResult<()>> + Send + 'a>> {
        Box::pin(async move { (self.0)(pool).await.map_err(Into::into) })
    }
}

static REGISTRY: Lazy<SchemaInitRegistry> = Lazy::new(|| Mutex::new(Vec::new()));

/// 注册在创建 pool 后统一调用的 schema 初始化逻辑（返回 `anyhow::Result<()>`）。
pub fn register_schema_init<N, F, Fut>(name: N, f: F)
where
    N: Into<String>,
    F: Fn(&SqlitePool) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = AnyhowResult<()>> + Send + 'static,
{
    REGISTRY
        .lock()
        .expect("schema registry mutex")
        .push((name.into(), Arc::new(ClosureSchemaInit(f))));
}

/// 注册返回 `Result<(), E>`（E: Into<anyhow::Error>）的 schema 初始化器，**可直接传入** core-sdk 的
/// `sqlite_init_schema` 等方法（无需再包一层 map_err）。
///
/// # 示例（直接使用 core-sdk 的 init_schema）
///
/// ```ignore
/// use flare_im_core_sdk_storage_sqlite::register_schema_init_with;
/// use flare_im_core_sdk::store::sqlite_init_schema;
///
/// register_schema_init_with("core", |pool| sqlite_init_schema(pool));
/// ```
pub fn register_schema_init_with<N, F, Fut, E>(name: N, f: F)
where
    N: Into<String>,
    F: Fn(&SqlitePool) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Into<anyhow::Error> + 'static,
{
    REGISTRY
        .lock()
        .expect("schema registry mutex")
        .push((name.into(), Arc::new(MapErrSchemaInit(f))));
}

/// 执行所有已注册的 schema 初始化器（内部使用）
pub async fn run_registered_schema_inits(pool: &SqlitePool) -> AnyhowResult<()> {
    let inits: SchemaInitSnapshot = REGISTRY.lock().expect("schema registry mutex").clone();
    for (name, init) in inits {
        init.run(pool)
            .await
            .map_err(|e| anyhow::anyhow!("schema init {:?} failed: {}", name, e))?;
    }
    Ok(())
}
