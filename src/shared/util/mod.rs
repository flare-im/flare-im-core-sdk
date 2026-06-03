pub mod constants;
pub mod date;
pub mod id;
pub mod paths;
#[cfg(feature = "lifecycle-sqlite")]
pub mod sqlite_store;
pub mod token;

pub const DEFAULT_TENANT_ID: &str = "0";

#[inline]
pub fn normalize_tenant_id(tenant_id: impl AsRef<str>) -> String {
    match tenant_id.as_ref().trim() {
        "" | "default" => DEFAULT_TENANT_ID.to_string(),
        tenant_id => tenant_id.to_string(),
    }
}

pub use constants::{
    RELIABLE_QUEUE_MAX_RETRIES, RELIABLE_QUEUE_TIMEOUT_SECS, REQUEST_TIMEOUT_SECS,
    WAIT_ACK_TIMEOUT_SECS,
};
pub use date::{
    ms_to_prost_timestamp, ms_to_rfc3339, prost_timestamp_to_ms, prost_timestamp_to_rfc3339,
    prost_timestamp_to_rfc3339_parts, rfc3339_to_prost_timestamp, system_time_to_prost_timestamp,
};
pub use paths::{
    default_sdk_data_root, dev_data_dir_relative_to_cwd, parse_data_url_to_path,
    resolve_sdk_data_root, resolve_user_db_path, resolve_user_media_cache_dir,
    sanitize_user_id_for_dir,
};
#[cfg(feature = "lifecycle-sqlite")]
pub use sqlite_store::{
    open_sqlite_store_for_user, open_sqlite_store_provider, sqlite_database_url_from_path,
};
pub use token::generate_test_token;
