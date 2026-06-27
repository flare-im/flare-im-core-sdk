//! [`IMClient`]：初始化、登录、登出与消息/会话 API（单一入口）。
//!
//! 内部为 [`tokio::sync::RwLock`]：同步读路径使用 `try_read`（禁止 `blocking_read`），避免在 `#[tokio::main]` 或异步任务中 panic。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};

use arc_swap::ArcSwapOption;
use tokio::sync::RwLock;

use crate::FlareError;
use crate::application::notification::NotificationHandlerRegistry;
use crate::application::services::LocalIdentityRepairService;
use crate::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi, ViewApi,
};
use crate::client::builder::{IMClientBuilder, IMClientExtensionComponents};
use crate::client::config::SdkResourceProfile;
use crate::client::connected_apis::ConnectedApis;
use crate::client::lifecycle::{SdkConfigOverlay, resolve_sdk_data_root};
use crate::extension::ExtensionRuntime;
use crate::extension::capability::SdkCapabilityRegistry;
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::transport::http::HttpRequestContext;
use crate::kernel::SdkState;
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::message::MessageLocalState;
use crate::runtime::SdkEngine;
use crate::shared::error::{ErrorCode, Result};
use crate::spi::metrics::{MetricsRecorder, MetricsSnapshot};
use flare_core::common::HeartbeatAppState;
use flare_proto::common::MessageStatus;
use rand::Rng;
use std::future::Future;
use std::time::Duration;

use crate::shared::util::spawn_background;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

const HEARTBEAT_APP_STATE_FOREGROUND: u8 = 0;
const HEARTBEAT_APP_STATE_BACKGROUND: u8 = 1;
const FOREGROUND_SYNC_INITIAL_DELAY_SECS: u64 = 1;
const FOREGROUND_SYNC_DESKTOP_INTERVAL_SECS: u64 = 2;
const FOREGROUND_SYNC_MOBILE_INTERVAL_SECS: u64 = 3;

mod async_accessors;
mod extension_lifecycle;
mod facades;
mod session_lifecycle;
mod session_watchers;
mod sync_api;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkChangeEvent {
    /// Whether the host currently has a usable network route.
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expensive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metered: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeHealthSnapshot {
    pub metrics_enabled: bool,
    pub state: SdkState,
    pub session_generation: u64,
    pub raw_subscriber_dropped_total: u64,
    pub metrics: MetricsSnapshot,
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_im_background<F>(future: F)
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn_background(async move {
        let _ = future.await;
    });
}

#[cfg(target_arch = "wasm32")]
fn spawn_im_background<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    spawn_background(future);
}

#[derive(Default)]
pub(crate) struct IMClientInner {
    pub environment: Option<String>,
    pub sdk_config: Option<SdkConfigOverlay>,
    pub data_root: Option<PathBuf>,
    pub current_user_id: Option<String>,
    pub connect_token: Option<String>,
    pub engine: Option<SdkEngine>,
    pub message_api: Option<MessageApi>,
    pub media_api: Option<Arc<MediaApi>>,
    pub capability_api: Option<Arc<CapabilityApi>>,
    pub presence_api: Option<Arc<PresenceApi>>,
    pub capability_registry: Option<Arc<SdkCapabilityRegistry>>,
    pub notification_registry: Option<Arc<NotificationHandlerRegistry>>,
    pub extension_runtime: Option<Arc<ExtensionRuntime>>,
    pub extension_components: IMClientExtensionComponents,
    pub message_build_api: Option<Arc<MessageBuildApi>>,
    pub conversation_api: Option<Arc<ConversationApi>>,
    pub view_api: Option<Arc<ViewApi>>,
    pub http_request_context: Option<Arc<HttpRequestContext>>,
    pub session_generation: u64,
    pub metrics: MetricsRecorder,
}

/// 唯一 SDK 句柄：[`Self::init`] → [`Self::login`]，或 [`Self::builder`] → [`Self::connect`]。
#[derive(Clone)]
pub struct IMClient {
    pub(crate) inner: Arc<RwLock<IMClientInner>>,
    session_snapshot: Arc<ArcSwapOption<ConnectedApis>>,
    state_snapshot: Arc<AtomicU8>,
    session_generation_snapshot: Arc<AtomicU64>,
    heartbeat_app_state_snapshot: Arc<AtomicU8>,
    network_reconnect_in_flight: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(crate) struct WeakIMClient {
    inner: Weak<RwLock<IMClientInner>>,
    session_snapshot: Weak<ArcSwapOption<ConnectedApis>>,
    state_snapshot: Weak<AtomicU8>,
    session_generation_snapshot: Weak<AtomicU64>,
    heartbeat_app_state_snapshot: Weak<AtomicU8>,
    network_reconnect_in_flight: Weak<AtomicBool>,
}

impl WeakIMClient {
    pub(crate) fn upgrade(&self) -> Option<IMClient> {
        Some(IMClient {
            inner: self.inner.upgrade()?,
            session_snapshot: self.session_snapshot.upgrade()?,
            state_snapshot: self.state_snapshot.upgrade()?,
            session_generation_snapshot: self.session_generation_snapshot.upgrade()?,
            heartbeat_app_state_snapshot: self.heartbeat_app_state_snapshot.upgrade()?,
            network_reconnect_in_flight: self.network_reconnect_in_flight.upgrade()?,
        })
    }
}

impl IMClient {
    /// 创建一个空的 SDK 客户端句柄。
    ///
    /// 仅创建内存中的客户端壳，不会触发网络连接或磁盘初始化；
    /// 需后续调用 [`Self::init`] + [`Self::login`]（或 builder 路径）进入可用态。
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(IMClientInner::default())),
            session_snapshot: Arc::new(ArcSwapOption::empty()),
            state_snapshot: Arc::new(AtomicU8::new(SdkState::Disconnected.as_u8())),
            session_generation_snapshot: Arc::new(AtomicU64::new(0)),
            heartbeat_app_state_snapshot: Arc::new(AtomicU8::new(HEARTBEAT_APP_STATE_FOREGROUND)),
            network_reconnect_in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 返回链式构建器，用于自定义存储、编解码器与中间件后再构建 [`IMClient`]。
    pub fn builder() -> IMClientBuilder {
        IMClientBuilder::new()
    }

    pub(crate) fn from_inner(inner: IMClientInner) -> Self {
        let generation = inner.session_generation;
        let state = inner
            .engine
            .as_ref()
            .map(|engine| engine.state())
            .unwrap_or(SdkState::Disconnected);
        Self {
            inner: Arc::new(RwLock::new(inner)),
            session_snapshot: Arc::new(ArcSwapOption::empty()),
            state_snapshot: Arc::new(AtomicU8::new(state.as_u8())),
            session_generation_snapshot: Arc::new(AtomicU64::new(generation)),
            heartbeat_app_state_snapshot: Arc::new(AtomicU8::new(HEARTBEAT_APP_STATE_FOREGROUND)),
            network_reconnect_in_flight: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn downgrade(&self) -> WeakIMClient {
        WeakIMClient {
            inner: Arc::downgrade(&self.inner),
            session_snapshot: Arc::downgrade(&self.session_snapshot),
            state_snapshot: Arc::downgrade(&self.state_snapshot),
            session_generation_snapshot: Arc::downgrade(&self.session_generation_snapshot),
            heartbeat_app_state_snapshot: Arc::downgrade(&self.heartbeat_app_state_snapshot),
            network_reconnect_in_flight: Arc::downgrade(&self.network_reconnect_in_flight),
        }
    }

    pub(crate) fn into_inner(self) -> Result<IMClientInner> {
        match Arc::try_unwrap(self.inner) {
            Ok(rw) => Ok(rw.into_inner()),
            Err(_) => Err(FlareError::localized(
                ErrorCode::InternalError,
                "IMClient must be uniquely owned",
            )),
        }
    }

    fn not_connected() -> FlareError {
        FlareError::localized(ErrorCode::NotConnected, "SDK not connected")
    }

    fn lock_busy() -> FlareError {
        FlareError::localized(ErrorCode::InternalError, "IMClient lock busy")
    }

    fn resolve_tenant_id(g: &IMClientInner) -> String {
        g.sdk_config
            .as_ref()
            .and_then(|c| c.tenant_id.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| std::env::var("FLARE_IM_TENANT_ID").ok())
            .map(crate::shared::util::normalize_tenant_id)
            .unwrap_or_else(|| "0".to_string())
    }

    fn store_state_snapshot(&self, state: SdkState) {
        self.state_snapshot.store(state.as_u8(), Ordering::Release);
    }

    fn load_state_snapshot(&self) -> SdkState {
        SdkState::from_u8(self.state_snapshot.load(Ordering::Acquire))
    }

    fn store_session_generation_snapshot(&self, generation: u64) {
        self.session_generation_snapshot
            .store(generation, Ordering::Release);
    }

    fn load_session_generation_snapshot(&self) -> u64 {
        self.session_generation_snapshot.load(Ordering::Acquire)
    }

    fn try_begin_network_reconnect(&self) -> bool {
        !self
            .network_reconnect_in_flight
            .swap(true, Ordering::AcqRel)
    }

    fn finish_network_reconnect(&self) {
        self.network_reconnect_in_flight
            .store(false, Ordering::Release);
    }

    fn store_heartbeat_app_state_snapshot(&self, state: HeartbeatAppState) {
        self.heartbeat_app_state_snapshot.store(
            match state {
                HeartbeatAppState::Foreground => HEARTBEAT_APP_STATE_FOREGROUND,
                HeartbeatAppState::Background => HEARTBEAT_APP_STATE_BACKGROUND,
            },
            Ordering::Release,
        );
    }

    fn is_app_foreground_snapshot(&self) -> bool {
        self.heartbeat_app_state_snapshot.load(Ordering::Acquire) == HEARTBEAT_APP_STATE_FOREGROUND
    }

    fn clear_session_snapshot(&self) {
        self.session_snapshot.store(None);
    }

    fn store_connected_apis_snapshot(&self, apis: ConnectedApis) {
        self.session_snapshot.store(Some(Arc::new(apis)));
    }

    fn active_connected_apis_snapshot(&self) -> Option<ConnectedApis> {
        if !Self::is_active_session_state(self.load_state_snapshot()) {
            return None;
        }
        self.session_snapshot
            .load_full()
            .map(|apis| (*apis).clone())
    }

    fn connected_apis_from_inner(g: &IMClientInner) -> Result<ConnectedApis> {
        Ok(ConnectedApis {
            message_api: g.message_api.clone().ok_or_else(Self::not_connected)?,
            conversation_api: g
                .conversation_api
                .as_ref()
                .map(|a| a.as_ref().clone())
                .ok_or_else(Self::not_connected)?,
            media_api: g.media_api.clone().ok_or_else(Self::not_connected)?,
            capability_api: g.capability_api.clone().ok_or_else(Self::not_connected)?,
            presence_api: g.presence_api.clone().ok_or_else(Self::not_connected)?,
            message_build_api: g
                .message_build_api
                .clone()
                .ok_or_else(Self::not_connected)?,
            view_api: g.view_api.clone().ok_or_else(Self::not_connected)?,
            capability_registry: g
                .capability_registry
                .clone()
                .ok_or_else(Self::not_connected)?,
        })
    }

    fn connected_apis_sync(&self) -> Result<ConnectedApis> {
        if let Some(apis) = self.active_connected_apis_snapshot() {
            return Ok(apis);
        }

        let g = self.read_active_inner()?;
        let apis = Self::connected_apis_from_inner(&g)?;
        drop(g);
        self.store_connected_apis_snapshot(apis.clone());
        Ok(apis)
    }

    fn is_active_session_state(state: SdkState) -> bool {
        matches!(
            state,
            SdkState::Connected | SdkState::Ready | SdkState::Reconnecting
        )
    }

    fn inner_session_active(&self, g: &IMClientInner) -> bool {
        let has_user = g.current_user_id.as_ref().is_some_and(|s| !s.is_empty());
        if !has_user {
            return false;
        }
        let state = g
            .engine
            .as_ref()
            .map(|engine| engine.state())
            .unwrap_or_else(|| self.load_state_snapshot());
        Self::is_active_session_state(state)
    }

    /// 同步 API 使用的读锁：在 Tokio worker 上 **禁止** `blocking_read`，必须用 `try_read`。
    pub(crate) fn read_inner(&self) -> Result<tokio::sync::RwLockReadGuard<'_, IMClientInner>> {
        self.inner.try_read().map_err(|_| Self::lock_busy())
    }

    fn read_active_inner(&self) -> Result<tokio::sync::RwLockReadGuard<'_, IMClientInner>> {
        let g = self.read_inner()?;
        if self.inner_session_active(&g) {
            Ok(g)
        } else {
            Err(Self::not_connected())
        }
    }

    /// 异步读锁：IPC 热路径应优先使用，避免 `try_read` 在写锁排队时返回 lock busy。
    pub(crate) async fn read_inner_async(
        &self,
    ) -> Result<tokio::sync::RwLockReadGuard<'_, IMClientInner>> {
        Ok(self.inner.read().await)
    }

    pub(crate) fn with_engine<R>(&self, f: impl FnOnce(&SdkEngine) -> R) -> Result<R> {
        let g = self.read_inner()?;
        let e = g.engine.as_ref().ok_or_else(Self::not_connected)?;
        Ok(f(e))
    }

    pub(crate) async fn with_engine_async<R>(&self, f: impl FnOnce(&SdkEngine) -> R) -> Result<R> {
        let g = self.read_inner_async().await?;
        let e = g.engine.as_ref().ok_or_else(Self::not_connected)?;
        Ok(f(e))
    }

    /// 登录成功后导出 Facade 快照，供 Tauri `SdkState` 缓存（避免每条 IPC 抢 `IMClient` 锁）。
    pub async fn connected_apis(&self) -> Result<ConnectedApis> {
        if let Some(apis) = self.active_connected_apis_snapshot() {
            return Ok(apis);
        }

        let g = self.read_inner_async().await?;
        if !self.inner_session_active(&g) {
            return Err(Self::not_connected());
        }
        let apis = Self::connected_apis_from_inner(&g)?;
        drop(g);
        self.store_connected_apis_snapshot(apis.clone());
        Ok(apis)
    }

    pub(super) async fn publish_session_event_if_generation(
        &self,
        generation: u64,
        event: SdkEvent,
    ) -> bool {
        let g = self.inner.read().await;
        if g.session_generation != generation {
            return false;
        }
        let Some(engine) = g.engine.as_ref() else {
            return false;
        };
        engine.bus().publish(event);
        true
    }

    /// 初始化运行环境与 SDK 配置快照。
    ///
    /// - 仅更新本地配置，不建连；
    /// - 若传入 `sdkConfig.dataUrl`，会解析为数据根；未传则使用 SDK 默认系统数据目录；
    /// - 会确保数据根存在，登录时按用户自动创建 SQLite 与媒体缓存目录；
    /// - 后续 [`Self::login`] 会基于该配置构建实际存储与连接参数。
    pub async fn init(
        &self,
        environment: Option<String>,
        sdk_config: Option<SdkConfigOverlay>,
    ) -> Result<()> {
        let mut g = self.inner.write().await;
        g.environment = environment;
        let data_root =
            resolve_sdk_data_root(sdk_config.as_ref().and_then(|cfg| cfg.data_url.as_deref()))?;
        #[cfg(not(target_arch = "wasm32"))]
        tokio::fs::create_dir_all(&data_root).await.map_err(|e| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                format!("sdk data root create_dir_all failed: {}", e),
            )
        })?;
        g.data_root = Some(data_root);
        g.sdk_config = sdk_config;
        Ok(())
    }

    /// 返回当前配置的 SDK 数据根目录（未传 `dataUrl` 时为 SDK 默认系统数据目录）。
    pub async fn data_root(&self) -> Option<PathBuf> {
        self.inner.read().await.data_root.clone()
    }

    /// 基于数据根目录解析一个子路径并确保父目录存在。
    ///
    /// 常用于上层保存附件、缓存或导出文件。若尚未 `init`，会使用 SDK 默认系统数据目录。
    pub async fn resolve_data_subpath(
        &self,
        relative: impl AsRef<std::path::Path>,
    ) -> Result<PathBuf> {
        let root = self
            .data_root()
            .await
            .unwrap_or_else(crate::shared::util::default_sdk_data_root);
        let p = root.join(relative.as_ref());
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(parent) = p.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                FlareError::localized(
                    ErrorCode::InvalidParameter,
                    format!("resolve_data_subpath create_dir_all failed: {}", e),
                )
            })?;
        }
        Ok(p)
    }

    /// 返回当前生效的 `(environment, sdkConfig)` 快照。
    pub async fn config_snapshot(&self) -> (Option<String>, Option<SdkConfigOverlay>) {
        let g = self.inner.read().await;
        (g.environment.clone(), g.sdk_config.clone())
    }

    /// Runtime health snapshot for diagnostics surfaces and platform SDKs.
    pub async fn runtime_health_snapshot(&self) -> RuntimeHealthSnapshot {
        let g = self.inner.read().await;
        RuntimeHealthSnapshot {
            metrics_enabled: g.metrics.is_enabled(),
            state: g
                .engine
                .as_ref()
                .map(|engine| engine.state())
                .unwrap_or_else(|| self.load_state_snapshot()),
            session_generation: g.session_generation,
            raw_subscriber_dropped_total: EventBus::raw_subscriber_dropped_total(),
            metrics: g.metrics.snapshot(),
        }
    }

    /// 登录专属队列重置：清空历史 pending，并将对应本地消息收敛为 failed。
    ///
    /// 设计目标：
    /// - 登录视为新会话边界，不继承历史待发队列；
    /// - 重连路径不触发该逻辑，仍保留同账号历史待发消息继续发送。
    async fn reset_pending_queue_on_login(&self) -> Result<()> {
        let mut should_publish_failed = true;
        let dropped_client_ids =
            if let Some(queue) = self.with_engine_async(|e| e.reliable_queue()).await? {
                // 由队列 actor 原子处理 in_flight + pending，避免与后台 tick 竞态。
                should_publish_failed = false;
                queue.reset_pending_on_login().await?
            } else {
                // 兜底分支：无可靠队列实现时，沿用仓储清理逻辑。
                let current_user_id = self.current_user_id().await.unwrap_or_default();
                if current_user_id.trim().is_empty() {
                    return Ok(());
                }
                let stores = self.stores_async().await?;
                let Some((pending_reader, pending_writer)) = stores.pending_sends() else {
                    return Ok(());
                };
                let pending_entries = pending_reader.list().await?;
                if pending_entries.is_empty() {
                    return Ok(());
                }
                let pending_ids = pending_entries
                    .iter()
                    .map(|entry| entry.client_msg_id.clone())
                    .collect::<Vec<_>>();
                let mut local_by_id = stores
                    .messages
                    .get_by_client_msg_ids(&pending_ids)
                    .await?
                    .into_iter()
                    .map(|message| (message.client_msg_id.clone(), message))
                    .collect::<HashMap<_, _>>();
                let mut dropped_client_ids = Vec::with_capacity(pending_entries.len());
                let mut failed_messages = Vec::new();
                for entry in pending_entries {
                    let _ = pending_writer.pop(&entry.client_msg_id).await?;
                    if let Some(mut local) = local_by_id.remove(&entry.client_msg_id) {
                        local.server_id = local.client_msg_id.clone();
                        local.local_state = MessageLocalState {
                            sending: false,
                            failed: true,
                            is_local: true,
                            uploading: false,
                            upload_progress: 0,
                            sort_ts: local.local_state.sort_ts,
                        };
                        local.status = MessageStatus::Failed as i32;
                        failed_messages.push(local);
                    }
                    dropped_client_ids.push(entry.client_msg_id);
                }
                if !failed_messages.is_empty() {
                    stores.messages.save_batch(&failed_messages).await?;
                }
                dropped_client_ids
            };

        if dropped_client_ids.is_empty() {
            return Ok(());
        }

        if !should_publish_failed {
            return Ok(());
        }

        let bus = self.bus().await?;
        for client_msg_id in dropped_client_ids {
            bus.publish(SdkEvent::Message(MessageEvent::SendFailed {
                client_msg_id,
                reason: "pending queue dropped during login session reset".to_string(),
            }));
        }
        Ok(())
    }

    async fn repair_local_conversation_identities_on_login(&self, user_id: &str) -> Result<()> {
        let stores = self.stores_async().await?;
        let report =
            LocalIdentityRepairService::new(stores.messages.clone(), stores.conversations.clone())
                .repair_single_chat_identities(user_id)
                .await?;
        if report.has_changes() {
            tracing::info!(
                target: "flare_sdk.identity",
                scanned_conversations = report.scanned_conversations,
                rewritten_conversations = report.rewritten_conversations,
                moved_messages = report.moved_messages,
                "repaired local conversation identities on login"
            );
        }
        Ok(())
    }

    /// 获取底层存储提供者（Message/Conversation/Cursor/PendingSend）。
    ///
    /// 通常用于诊断、调试或高级扩展场景。
    pub fn stores(&self) -> Result<StoreProvider> {
        self.with_engine(|e| e.stores().clone())
    }

    pub async fn stores_async(&self) -> Result<StoreProvider> {
        self.with_engine_async(|e| e.stores().clone()).await
    }
}

fn should_skip_reconnect_for_disconnect_reason(reason: &str) -> bool {
    let lower = reason.trim().to_lowercase();
    lower.contains("client disconnected")
        || lower.contains("closed by client")
        || lower.contains("reconnect attempts exhausted")
        || lower.contains("kick")
        || lower.contains("设备冲突")
        || lower.contains("device_conflict")
        || lower.contains("token_expired")
        || lower.contains("token expired")
        || lower.contains("401")
        || lower.contains("credential expired")
}

fn foreground_sync_interval_for_profile(profile: Option<SdkResourceProfile>) -> Duration {
    match profile.unwrap_or(SdkResourceProfile::Mobile) {
        SdkResourceProfile::Desktop => Duration::from_secs(FOREGROUND_SYNC_DESKTOP_INTERVAL_SECS),
        SdkResourceProfile::Mobile => Duration::from_secs(FOREGROUND_SYNC_MOBILE_INTERVAL_SECS),
    }
}

fn reconnect_delay_secs(base_interval_secs: u64, attempt: u32) -> u64 {
    let (min, max) = reconnect_delay_bounds_secs(base_interval_secs, attempt);
    if min == max {
        return min;
    }
    rand::thread_rng().gen_range(min..=max)
}

fn reconnect_delay_bounds_secs(base_interval_secs: u64, attempt: u32) -> (u64, u64) {
    let shift = attempt.saturating_sub(1).min(4);
    let nominal = base_interval_secs
        .saturating_mul(1_u64 << shift)
        .clamp(1, 30);
    let min = nominal.saturating_mul(80).div_ceil(100).max(1);
    let max = nominal.saturating_mul(120).div_ceil(100).clamp(min, 30);
    (min, max)
}

impl Default for IMClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
