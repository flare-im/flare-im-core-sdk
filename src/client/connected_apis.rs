//! 登录成功后导出的 API 句柄快照，供 Tauri 等高频 IPC 无锁读取。

use std::sync::Arc;

use crate::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi,
};
use crate::extension::capability::SdkCapabilityRegistry;

/// 已连接会话的 Facade 克隆集合（不持有 `IMClient` 全局写锁）。
#[derive(Clone)]
pub struct ConnectedApis {
    pub message_api: MessageApi,
    pub conversation_api: ConversationApi,
    pub media_api: Arc<MediaApi>,
    pub capability_api: Arc<CapabilityApi>,
    pub presence_api: Arc<PresenceApi>,
    pub message_build_api: Arc<MessageBuildApi>,
    pub capability_registry: Arc<SdkCapabilityRegistry>,
}
