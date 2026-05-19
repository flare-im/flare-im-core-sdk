//! SQLite `StoreProvider` 构建（`feature = "lifecycle-sqlite"`）。
//!
//! 与 [`crate::client::IMClient::login`] 配合：在 `LoginDbKind::Sqlite` 分支由 `im_client` 调用 [`open_sqlite_store_for_user`]；
//! 也可直接用 [`open_sqlite_store_provider`] 传入任意 `database_url`。

use std::sync::{Arc, OnceLock};

use flare_im_core_sdk_storage_sqlite::{
    database_url_from_path, open_pool, register_schema_init_with,
};

use crate::FlareError;
use crate::Result;
use crate::domain::{MediaCacheAdmin, MediaCacheStore, UserFileDownloadStore};
use crate::error::ErrorCode;
use crate::store::{
    SqliteConversationParticipantRepo, SqliteConversationRepo, SqliteMediaCacheRepo,
    SqliteMessageRepo, SqlitePendingSendRepo, SqliteSyncCursorRepo, SqliteUploadManifestRepo,
    SqliteUserFileDownloadRepo, SqliteUserProfileRepo, StoreProvider, sqlite_init_schema,
};
use crate::util::paths::{resolve_media_cache_dir_next_to_db, resolve_user_db_path};

static CORE_SCHEMA: OnceLock<()> = OnceLock::new();

/// 注册 IM 核心 SQLite 表结构（与社交 schema 一并注册后，共享同一 `open_pool` 连接）。
pub fn ensure_core_sqlite_schema_registered() {
    CORE_SCHEMA.get_or_init(|| {
        register_schema_init_with("flare_im_core_schema", |pool| {
            let pool = pool.clone();
            async move {
                sqlite_init_schema(&pool)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
            }
        });
    });
}

/// `sqlite:` 连接串，供 [`open_sqlite_store_provider`]。
#[inline]
pub fn sqlite_database_url_from_path(path: &std::path::Path) -> String {
    database_url_from_path(path)
}

/// 打开连接池并组装核心 SDK 所需的 SQLite 仓储集合。
///
/// `media_cache_dir`：媒体文件落盘根目录；为 `None` 时不启用本地媒体缓存（`resolve_media_access` 始终走网关）。
pub async fn open_sqlite_store_provider(
    database_url: &str,
    media_cache_dir: Option<&std::path::Path>,
) -> Result<StoreProvider> {
    ensure_core_sqlite_schema_registered();
    let pool = open_pool(database_url).await.map_err(|e| {
        FlareError::localized(
            ErrorCode::ConfigurationError,
            format!("open_pool failed: {}", e),
        )
    })?;

    let pending_repo = Arc::new(SqlitePendingSendRepo::new(pool.clone()));
    let upload_manifest_repo = Arc::new(SqliteUploadManifestRepo::new(pool.clone()));
    let user_download_repo = Arc::new(SqliteUserFileDownloadRepo::new(pool.clone()));
    let user_repo = Arc::new(SqliteUserProfileRepo::new(pool.clone()));
    let (media_cache_store, media_cache_admin) = if let Some(root) = media_cache_dir {
        let repo = Arc::new(SqliteMediaCacheRepo::create(pool.clone(), root.to_path_buf()).await?);
        (
            Some(repo.clone() as Arc<dyn MediaCacheStore>),
            Some(repo as Arc<dyn MediaCacheAdmin>),
        )
    } else {
        (None, None)
    };
    Ok(StoreProvider {
        messages: Arc::new(SqliteMessageRepo::new(pool.clone())),
        conversations: Arc::new(SqliteConversationRepo::new(pool.clone())),
        conversation_participants: Some(Arc::new(SqliteConversationParticipantRepo::new(
            pool.clone(),
        ))),
        cursors: Arc::new(SqliteSyncCursorRepo::new(pool.clone())),
        pending_send_reader: Some(pending_repo.clone()),
        pending_send_writer: Some(pending_repo),
        upload_manifest_store: Some(upload_manifest_repo),
        media_cache_store,
        media_cache_admin,
        user_file_download_store: Some(user_download_repo as Arc<dyn UserFileDownloadStore>),
        user_profiles_reader: Some(user_repo.clone()),
        user_profiles_writer: Some(user_repo),
    })
}

/// 在 `data_root` 下按用户目录打开库（路径规则见 [`crate::util::paths::resolve_user_db_path`]）。
pub async fn open_sqlite_store_for_user(
    base_data_dir: &std::path::Path,
    user_id: &str,
) -> Result<StoreProvider> {
    let db_path = resolve_user_db_path(base_data_dir, user_id);
    let cache_dir = resolve_media_cache_dir_next_to_db(&db_path);
    tracing::info!(db = %db_path.display(), cache = %cache_dir.display(), "Opening SQLite store");
    let database_url = sqlite_database_url_from_path(&db_path);
    open_sqlite_store_provider(&database_url, Some(&cache_dir)).await
}
