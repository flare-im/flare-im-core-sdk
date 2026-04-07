//! 客户端壳层：配置与生命周期、[`IMClient`]、消息/会话 Facade（[`api`]）、引擎组装（[`builder`]）、事件订阅（[`events`]）。
//!
//! 与「消息/会话领域逻辑」的分界：业务命令在 [`api`] → `application`；此处只做句柄、连接与 `EventBus` 订阅入口。

pub mod api;
pub mod builder;
pub mod config;
pub mod events;
pub mod im_client;
pub mod lifecycle;

pub use api::{
    ConversationApi, MediaApi, MessageApi, MessageBuildApi, UploadPhase, UploadProgress,
    UploadProgressCallback,
};
pub use builder::IMClientBuilder;
pub use config::{SdkConfig, SdkConfigBuilder};
pub use im_client::IMClient;
pub use crate::model::{
    MediaAccessUrl, MediaCacheEntryVo, MediaCacheStatsVo, MediaResolvedAccess, UploadOptions,
    UploadedMedia,
};
pub use lifecycle::{
    default_ws_url, dev_data_dir_relative_to_cwd, merge_sdk_config, parse_data_url_to_path,
    resolve_connect_token, resolve_user_db_path, sanitize_user_id_for_dir, LoginDbKind,
    SdkConfigOverlay,
};
