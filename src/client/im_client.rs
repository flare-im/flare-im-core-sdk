//! [`IMClient`]：初始化、登录、登出与消息/会话 API（单一入口）。
//!
//! 内部为 [`tokio::sync::RwLock`]：同步读路径使用 `try_read`（禁止 `blocking_read`），避免在 `#[tokio::main]` 或异步任务中 panic。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::FlareError;
use crate::application::notification::NotificationHandlerRegistry;
use crate::client::api::{
    CapabilityApi, CapabilityDispatchResult, ConversationApi, MediaApi, MessageApi,
    MessageBuildApi, PresenceApi, UserCapabilityGrantDto, UserPresenceDto,
};
use crate::client::builder::{IMClientBuilder, IMClientExtensionComponents};
use crate::client::connected_apis::ConnectedApis;
use crate::client::lifecycle::{
    LoginDbKind, SdkConfigOverlay, default_ws_url, merge_sdk_config, resolve_connect_token,
    resolve_sdk_data_root,
};
use crate::core::event::{ConnectionEvent, EventBus, MessageEvent, SdkEvent};
use crate::core::{SdkEngine, SdkState, SyncRunContext};
use crate::extension::capability::SdkCapabilityRegistry;
use crate::extension::{ExtensionContext, ExtensionLifecycleContext, ExtensionRuntime};
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::transport::http::HttpRequestContext;
use crate::model::message::MessageLocalState;
use crate::model::{
    ConversationVersion, SyncConversationSummariesRequest, SyncConversationSummariesResponse,
};
use crate::shared::error::{ErrorCode, Result};
use crate::shared::util::{
    CoreTokenConfig, delay, generate_core_token as util_generate_core_token, timeout,
};
use flare_core::common::{HeartbeatAppState, HeartbeatConfig};
use flare_proto::common::{CapabilityPacket, MessageStatus};
use rand::Rng;
use serde_json::Value;
use std::future::Future;
use std::time::Duration;

use crate::shared::util::spawn_background;
use std::sync::atomic::{AtomicU8, Ordering};

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
    pub http_request_context: Option<Arc<HttpRequestContext>>,
    pub session_generation: u64,
}

/// 唯一 SDK 句柄：[`Self::init`] → [`Self::login`]，或 [`Self::builder`] → [`Self::connect`]。
#[derive(Clone)]
pub struct IMClient {
    pub(crate) inner: Arc<RwLock<IMClientInner>>,
    state_snapshot: Arc<AtomicU8>,
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
            state_snapshot: Arc::new(AtomicU8::new(SdkState::Disconnected.as_u8())),
        }
    }

    /// 返回链式构建器，用于自定义存储、编解码器与中间件后再构建 [`IMClient`]。
    pub fn builder() -> IMClientBuilder {
        IMClientBuilder::new()
    }

    pub(crate) fn from_inner(inner: IMClientInner) -> Self {
        let state = inner
            .engine
            .as_ref()
            .map(|engine| engine.state())
            .unwrap_or(SdkState::Disconnected);
        Self {
            inner: Arc::new(RwLock::new(inner)),
            state_snapshot: Arc::new(AtomicU8::new(state.as_u8())),
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
        let g = self.read_inner_async().await?;
        if !self.inner_session_active(&g) {
            return Err(Self::not_connected());
        }
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
            capability_registry: g
                .capability_registry
                .clone()
                .ok_or_else(Self::not_connected)?,
        })
    }

    /// 初始化运行环境与 SDK 配置快照。
    ///
    /// - 仅更新本地配置，不建连；
    /// - 若传入 `sdk_config.data_url`，会解析为数据根；未传则使用 SDK 默认系统数据目录；
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

    /// 返回当前生效的 `(environment, sdk_config)` 快照。
    pub async fn config_snapshot(&self) -> (Option<String>, Option<SdkConfigOverlay>) {
        let g = self.inner.read().await;
        (g.environment.clone(), g.sdk_config.clone())
    }

    /// 是否处于“已登录 SDK 会话”状态。
    ///
    /// 该值表示本地有当前用户上下文，不等同于底层链路一定可发包；
    /// 连接细粒度状态请使用 [`Self::state`] 或事件回调。
    pub async fn is_connected(&self) -> bool {
        self.inner.read().await.current_user_id.is_some()
    }

    /// 运行期替换心跳策略。未连接时 no-op，便于平台生命周期回调早于 login 到达。
    pub async fn update_heartbeat_config(&self, config: HeartbeatConfig) -> Result<()> {
        let g = self.read_inner_async().await?;
        if let Some(engine) = g.engine.as_ref() {
            engine.update_heartbeat_config(config).await?;
        }
        Ok(())
    }

    /// 更新应用前后台状态。移动端进入后台时可拉长心跳，回到前台时恢复较短心跳。
    pub async fn set_heartbeat_app_state(&self, state: HeartbeatAppState) -> Result<()> {
        let g = self.read_inner_async().await?;
        if let Some(engine) = g.engine.as_ref() {
            engine.set_heartbeat_app_state(state).await?;
        }
        Ok(())
    }

    /// 更新 NAT 空闲超时探测结果。传入 `None` 表示清除探测值。
    pub async fn set_heartbeat_nat_timeout(&self, timeout: Option<Duration>) -> Result<()> {
        let g = self.read_inner_async().await?;
        if let Some(engine) = g.engine.as_ref() {
            engine.set_heartbeat_nat_timeout(timeout).await?;
        }
        Ok(())
    }

    /// 当前实际心跳间隔；未连接时返回 `None`。
    pub async fn heartbeat_effective_interval(&self) -> Result<Option<Duration>> {
        let g = self.read_inner_async().await?;
        match g.engine.as_ref() {
            Some(engine) => Ok(engine.heartbeat_effective_interval().await),
            None => Ok(None),
        }
    }

    /// 与 [`Self::state`] / 传输层 `Ready` 不同：未登录时引擎可能仍存在，此时本方法为 `false`。
    pub fn session_active_sync(&self) -> bool {
        self.inner
            .try_read()
            .map(|g| self.inner_session_active(&g))
            .unwrap_or(false)
    }

    /// 返回当前登录用户 ID；未登录时返回 `None`。
    pub async fn current_user_id(&self) -> Option<String> {
        self.inner.read().await.current_user_id.clone()
    }

    /// 会话代际：登录/重连/登出时递增，供 Tauri `SdkState` 判断 API 快照是否过期。
    pub async fn session_generation(&self) -> u64 {
        self.inner.read().await.session_generation
    }

    /// 生成 Flare IM Core 接入 JWT token。
    ///
    /// 调用方必须显式传入签名 secret、issuer、user_id 与 ttl，避免默认测试密钥进入真实集成。
    pub fn generate_core_token(config: CoreTokenConfig) -> Result<String> {
        util_generate_core_token(&config)
    }

    /// 主动退出登录并清空 SDK 会话上下文。
    ///
    /// 该操作会断开连接、推进会话代际并清理 `message/conversation` API 句柄；
    /// 调用后需重新 `login` 或 `connect` 才可继续收发消息。
    pub async fn logout(&self) -> Result<()> {
        let lifecycle = self.extension_lifecycle_snapshot(None).await;
        let (engine, presence_api, presence_user_id, http_request_context) = {
            let mut g = self.inner.write().await;
            let presence_api = g.presence_api.clone();
            let presence_user_id = g.current_user_id.clone();
            g.session_generation = g.session_generation.wrapping_add(1);
            g.current_user_id = None;
            g.connect_token = None;
            g.message_api = None;
            g.media_api = None;
            g.capability_api = None;
            g.presence_api = None;
            g.capability_registry = None;
            g.message_build_api = None;
            g.conversation_api = None;
            let engine = g.engine.take();
            let http_request_context = g.http_request_context.clone();
            (engine, presence_api, presence_user_id, http_request_context)
        };
        self.store_state_snapshot(SdkState::Disconnected);
        if let Some(engine) = engine.as_ref() {
            engine.deactivate_local_session().await;
        }
        if let Some(api) = presence_api.as_ref() {
            let logout_result = if let Some(user_id) = presence_user_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
            {
                api.logout_user_device_presence(user_id).await
            } else {
                api.logout_current_device_presence().await
            };
            if let Err(err) = logout_result {
                tracing::warn!(%err, "active presence logout failed; falling back to transport disconnect");
            }
        }
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(String::new(), None).await;
            context.clear_gateway_context().await;
        }
        if let Some(mut e) = engine {
            e.disconnect().await?;
        }
        Self::notify_extension_disconnect_snapshot(&lifecycle).await;
        Self::notify_extension_logout_snapshot(&lifecycle).await;
        Ok(())
    }

    /// 反初始化 SDK 运行态。
    ///
    /// 与 [`Self::logout`] 的区别：`logout` 只结束当前用户会话并保留 `init` 配置；
    /// `uninit` 会在结束会话后清空环境、配置、数据根与宿主 HTTP 上下文，允许同一 client
    /// 以新的 `SdkConfigOverlay` 重新 [`Self::init`]。
    pub async fn uninit(&self) -> Result<()> {
        let logout_result = self.logout().await;
        {
            let mut g = self.inner.write().await;
            g.environment = None;
            g.sdk_config = None;
            g.data_root = None;
            g.http_request_context = None;
            g.notification_registry = None;
        }
        logout_result
    }

    /// 登录前清理旧会话：presence / disconnect 带短超时，避免 Consul 或 gRPC 阻塞 `sdk_login`。
    async fn logout_for_login(&self) -> Result<()> {
        const PRESENCE_LOGOUT_TIMEOUT: Duration = Duration::from_secs(2);
        const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(3);

        let lifecycle = self.extension_lifecycle_snapshot(None).await;
        let (engine, presence_api, presence_user_id, http_request_context) = {
            let mut g = self.inner.write().await;
            let presence_api = g.presence_api.clone();
            let presence_user_id = g.current_user_id.clone();
            g.session_generation = g.session_generation.wrapping_add(1);
            g.current_user_id = None;
            g.connect_token = None;
            g.message_api = None;
            g.media_api = None;
            g.capability_api = None;
            g.presence_api = None;
            g.capability_registry = None;
            g.message_build_api = None;
            g.conversation_api = None;
            let engine = g.engine.take();
            let http_request_context = g.http_request_context.clone();
            (engine, presence_api, presence_user_id, http_request_context)
        };
        self.store_state_snapshot(SdkState::Disconnected);
        if let Some(engine) = engine.as_ref() {
            engine.deactivate_local_session().await;
        }

        if let Some(api) = presence_api.as_ref() {
            let logout_future = async {
                if let Some(user_id) = presence_user_id
                    .as_deref()
                    .filter(|id| !id.trim().is_empty())
                {
                    api.logout_user_device_presence(user_id).await
                } else {
                    api.logout_current_device_presence().await
                }
            };
            match timeout(PRESENCE_LOGOUT_TIMEOUT, logout_future).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    tracing::warn!(%err, "presence logout before login failed");
                }
                Err(_) => tracing::warn!("presence logout before login timed out"),
            }
        }
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(String::new(), None).await;
            context.clear_gateway_context().await;
        }
        if let Some(mut e) = engine {
            match timeout(DISCONNECT_TIMEOUT, e.disconnect()).await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::warn!(%err, "disconnect before login failed"),
                Err(_) => tracing::warn!("disconnect before login timed out"),
            }
        }
        Self::notify_extension_disconnect_snapshot(&lifecycle).await;
        Self::notify_extension_logout_snapshot(&lifecycle).await;
        Ok(())
    }

    /// 登录入口：按用户初始化存储、建立连接并切换为新会话。
    ///
    /// - 会先执行一次 [`Self::logout`]，确保会话隔离；
    /// - `before_connect` 可在建连前注册事件监听；
    /// - 被踢下线 / token 过期将由内部 watcher 自动终止会话（等价登出）。
    /// 准备登录会话：打开 per-user 本地库、装配引擎，但**不连网**。
    ///
    /// 配合 [`Self::connect`] 实现「初始化前置、登录只做网络」：App 启动即可对
    /// 「上次登录用户」调用 `prepare`，把开库 / 迁移 / 建引擎 / 待发队列恢复等本地重活
    /// 移出登录关键路径；待拿到 token 再 [`Self::connect`]，登录仅剩连接 + 首次同步。
    ///
    /// 幂等：对任意 user 重复调用都会先清理旧会话、丢弃未连接引擎，再按新 user 重建本地栈。
    pub async fn prepare(&self, user_id: &str, db: LoginDbKind) -> Result<()> {
        let snap = {
            let g = self.inner.read().await;
            (
                g.environment.clone(),
                g.sdk_config.clone(),
                g.data_root.clone(),
                g.http_request_context.clone(),
                g.extension_components.clone(),
            )
        };
        self.logout_for_login().await?;
        let data_root = match snap.2.clone() {
            Some(path) => path,
            None => {
                let path =
                    resolve_sdk_data_root(snap.1.as_ref().and_then(|cfg| cfg.data_url.as_deref()))?;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::fs::create_dir_all(&path).await.map_err(|e| {
                    FlareError::localized(
                        ErrorCode::InvalidParameter,
                        format!("sdk data root create_dir_all failed: {}", e),
                    )
                })?;
                path
            }
        };
        #[cfg(feature = "lifecycle-sqlite")]
        let stores = match db {
            #[cfg(feature = "lifecycle-sqlite")]
            LoginDbKind::Sqlite => {
                crate::shared::util::sqlite_store::open_sqlite_store_for_user(&data_root, user_id)
                    .await?
            }
            LoginDbKind::IndexedDb(stores) => stores,
        };
        #[cfg(not(feature = "lifecycle-sqlite"))]
        let LoginDbKind::IndexedDb(stores) = db;
        let ws_url = default_ws_url(snap.1.as_ref());
        let config = merge_sdk_config(&ws_url, snap.1.as_ref());
        let mut child_builder = IMClientBuilder::new().config(config).stores(stores);
        if let Some(ctx) = snap.3.clone() {
            child_builder = child_builder.http_request_context(ctx);
        }
        child_builder = snap.4.apply_to_builder(child_builder);
        let child = child_builder.build()?;
        let mut inner = child.into_inner()?;
        inner.environment = snap.0;
        inner.sdk_config = snap.1;
        inner.data_root = Some(data_root);
        if inner.http_request_context.is_none() {
            inner.http_request_context = snap.3;
        }
        inner.current_user_id = Some(user_id.to_string());
        *self.inner.write().await = inner;
        tokio::task::yield_now().await;
        self.reset_pending_queue_on_login().await?;
        self.notify_extension_login_best_effort(user_id).await;
        Ok(())
    }

    /// 一步登录：[`Self::prepare`] + [`Self::connect`] 的组合，供不做预热的调用方使用。
    ///
    /// `before_connect` 在 `prepare` 之后、建连之前回调，可在此注册事件监听
    /// （预热路径下等价于在 `prepare` 与 `connect` 之间订阅事件）。
    pub async fn login<F>(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
        db: LoginDbKind,
        before_connect: F,
    ) -> Result<ConnectedApis>
    where
        F: FnOnce(crate::core::event::EventBus, Arc<dyn crate::domain::MessageStore>)
            + Send
            + 'static,
    {
        self.prepare(user_id, db).await?;
        let bus = self.bus().await?.clone();
        let msg_store = self.stores_async().await?.messages.clone();
        before_connect(bus, msg_store);
        self.connect(user_id, explicit_token).await
    }

    /// 连接已 [`Self::prepare`] / builder 装配好的引擎并完成首次同步，返回连接态 API 快照。
    ///
    /// 「预热后登录」的网络半段：先 [`Self::prepare`]，拿到 token 后调用本方法即可，
    /// 登录关键路径只剩连接握手 + 首次同步。若引擎不存在（未 prepare / 未 build）返回 `NotConnected`。
    pub async fn connect(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
    ) -> Result<ConnectedApis> {
        self.connect_internal(user_id, explicit_token, true).await?;
        self.connected_apis().await
    }

    async fn connect_internal(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
        install_watcher: bool,
    ) -> Result<()> {
        let token = resolve_connect_token(user_id, explicit_token)?;
        let (engine, http_request_context) = {
            let mut g = self.inner.write().await;
            (g.engine.take(), g.http_request_context.clone())
        };
        let mut e = engine.ok_or_else(|| {
            FlareError::localized(ErrorCode::NotConnected, "no engine; use builder or login")
        })?;
        self.store_state_snapshot(SdkState::Connecting);
        if let Err(error) = e.connect(user_id, &token).await {
            self.store_state_snapshot(e.state());
            let mut g = self.inner.write().await;
            if g.engine.is_none() {
                g.engine = Some(e);
            }
            return Err(error);
        }
        self.store_state_snapshot(e.state());
        let bus = e.bus().clone();
        let mut g = self.inner.write().await;
        g.engine = Some(e);
        g.current_user_id = Some(user_id.to_string());
        g.connect_token = Some(token.clone());
        g.session_generation = g.session_generation.wrapping_add(1);
        let current_generation = g.session_generation;
        let tenant_id = Self::resolve_tenant_id(&g);
        drop(g);
        // 仅写入 IM token；Social Gateway Bearer 由 apply_gateway_session / ensure_gateway_auth 维护。
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(token.clone(), None).await;
            // IM login 期间 Background 社交同步仍走共享 HTTP，须保留 user/tenant。
            context.ensure_identity(user_id, &tenant_id).await;
        }
        self.notify_extension_connect_best_effort(user_id).await;
        if install_watcher {
            self.spawn_state_snapshot_watcher(current_generation, bus.clone());
            self.spawn_terminal_session_watcher(current_generation, bus.clone());
            self.spawn_reconnect_session_watcher(current_generation, bus);
        }
        Ok(())
    }

    /// 主动断开连接并清理当前会话上下文（语义上等价于轻量登出）。
    pub async fn disconnect(&self) -> Result<()> {
        let lifecycle = self.extension_lifecycle_snapshot(None).await;
        let (engine, http_request_context) = {
            let mut g = self.inner.write().await;
            g.session_generation = g.session_generation.wrapping_add(1);
            g.current_user_id = None;
            g.connect_token = None;
            g.message_api = None;
            g.media_api = None;
            g.capability_api = None;
            g.presence_api = None;
            g.capability_registry = None;
            g.message_build_api = None;
            g.conversation_api = None;
            let engine = g.engine.take();
            let http_request_context = g.http_request_context.clone();
            (engine, http_request_context)
        };
        self.store_state_snapshot(SdkState::Disconnected);
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(String::new(), None).await;
            context.clear_gateway_context().await;
        }
        if let Some(mut e) = engine {
            e.disconnect().await?;
        }
        Self::notify_extension_disconnect_snapshot(&lifecycle).await;
        Ok(())
    }

    /// 读取 SDK 当前连接状态快照（FSM 驱动）。
    ///
    /// 锁竞争或引擎暂时被连接/重连流程取出时，返回句柄级状态快照。
    pub fn state(&self) -> SdkState {
        match self.inner.try_read() {
            Ok(g) => match g.engine.as_ref() {
                Some(engine) => {
                    let state = engine.state();
                    self.store_state_snapshot(state);
                    state
                }
                None => self.load_state_snapshot(),
            },
            Err(_) => self.load_state_snapshot(),
        }
    }

    /// 共享 HTTP 鉴权上下文（媒体 / 能力 / Social Gateway 可共用）。
    pub fn http_request_context(&self) -> Option<Arc<HttpRequestContext>> {
        self.inner
            .try_read()
            .ok()
            .and_then(|g| g.http_request_context.clone())
    }

    /// 当前 IM 连接使用的 access token（与 WebSocket 鉴权一致）。
    pub async fn access_token(&self) -> Option<String> {
        let g = self.inner.read().await;
        g.connect_token.clone().filter(|t| !t.trim().is_empty())
    }

    /// 将 IM 会话 token 写入共享 HTTP 上下文（Social Gateway / 媒体 / 能力 API 的 Bearer）。
    pub async fn sync_gateway_http_context(&self, tenant_id: Option<&str>) -> Result<()> {
        let g = self.inner.read().await;
        if !self.inner_session_active(&g) {
            return Err(Self::not_connected());
        }
        let user_id = g
            .current_user_id
            .clone()
            .filter(|u| !u.is_empty())
            .ok_or_else(Self::not_connected)?;
        let token = g
            .connect_token
            .clone()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(Self::not_connected)?;
        let tenant = tenant_id
            .map(str::to_string)
            .unwrap_or_else(|| Self::resolve_tenant_id(&g));
        let tenant = crate::shared::util::normalize_tenant_id(tenant);
        let http = g.http_request_context.clone().ok_or_else(|| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                "http_request_context not configured",
            )
        })?;
        drop(g);
        http.set_gateway_context(token, tenant, user_id, None).await;
        Ok(())
    }

    /// 更新 access token 并同步到共享 HTTP 上下文（token 刷新后调用）。
    pub async fn update_access_token(
        &self,
        access_token: impl Into<String>,
        tenant_id: Option<&str>,
    ) -> Result<()> {
        let token = access_token.into();
        if token.trim().is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "access_token must not be empty",
            ));
        }
        {
            let mut g = self.inner.write().await;
            g.connect_token = Some(token.clone());
        }
        self.sync_gateway_http_context(tenant_id).await
    }

    /// 获取消息 API 门面；未连接时返回 `NotConnected`。
    pub fn message(&self) -> Result<MessageApi> {
        self.read_active_inner()?
            .message_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取消息构建 API（负责组装 `IMMessage`）；未连接时返回 `NotConnected`。
    pub fn message_build(&self) -> Result<Arc<MessageBuildApi>> {
        self.read_active_inner()?
            .message_build_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取会话 API 门面；未连接时返回 `NotConnected`。
    pub fn conversation(&self) -> Result<ConversationApi> {
        self.read_active_inner()?
            .conversation_api
            .as_ref()
            .map(|a| a.as_ref().clone())
            .ok_or_else(Self::not_connected)
    }

    /// 获取媒体 API 门面（上传/删除）。
    pub fn media(&self) -> Result<Arc<MediaApi>> {
        self.read_active_inner()?
            .media_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取能力插件 API（付费模块入口，包含 RTC/SFU 能力）。
    pub fn capability(&self) -> Result<Arc<CapabilityApi>> {
        self.read_active_inner()?
            .capability_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取用户在线状态 API。
    pub fn presence(&self) -> Result<Arc<PresenceApi>> {
        self.read_active_inner()?
            .presence_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取 SDK 能力插件注册表（支持多付费插件扩展）。
    pub fn capability_registry(&self) -> Result<Arc<SdkCapabilityRegistry>> {
        self.read_active_inner()?
            .capability_registry
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 经注册表派发扩展能力（等价于 `capability_registry()?.invoke(...).await`）。
    pub async fn invoke_capability(
        &self,
        capability_id: &str,
        payload: Value,
        conversation_id: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Result<CapabilityDispatchResult> {
        self.capability_registry()?
            .invoke(capability_id, payload, conversation_id, tenant_id)
            .await
    }

    /// 经注册表查询某 `capability_id` 所属命名空间插件的用户授权列表。
    pub async fn list_capability_grants(
        &self,
        capability_id: &str,
        tenant_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<UserCapabilityGrantDto>> {
        self.capability_registry()?
            .list_user_grants_for_capability(capability_id, tenant_id, user_id)
            .await
    }

    /// 上行发送能力包（DATA capability，不占用 conversation_seq）。
    pub async fn send_capability_packet(&self, packet: CapabilityPacket) -> Result<()> {
        let sender = self.with_engine_async(|e| e.sender().clone()).await?;
        sender.send_capability_packet(&packet).await
    }

    /// 获取 SDK 事件总线（用于原始事件订阅或桥接到宿主事件系统）。
    pub async fn bus(&self) -> Result<EventBus> {
        self.with_engine_async(|e| e.bus().clone()).await
    }

    pub async fn notification_handlers(&self) -> Result<Arc<NotificationHandlerRegistry>> {
        let g = self.read_inner_async().await?;
        g.notification_registry
            .clone()
            .ok_or_else(|| FlareError::localized(ErrorCode::InternalError, "IMClient not built"))
    }

    /// 同步获取事件总线：仅用于非 async 上下文；热路径请用 [`Self::bus`].
    pub fn bus_sync(&self) -> Result<EventBus> {
        self.with_engine(|e| e.bus().clone())
    }

    fn spawn_state_snapshot_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.clone();
        let mut rx = bus.subscribe_raw();
        spawn_im_background(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(_) => break,
                };
                if !client.is_generation_current(generation).await {
                    break;
                }
                match event {
                    SdkEvent::Connection(ConnectionEvent::StateChanged { state }) => {
                        client.store_state_snapshot(state);
                    }
                    SdkEvent::Connection(ConnectionEvent::Disconnected { .. })
                    | SdkEvent::Connection(ConnectionEvent::KickedOff { .. })
                    | SdkEvent::Connection(ConnectionEvent::TokenExpired { .. }) => {
                        client.store_state_snapshot(SdkState::Disconnected);
                    }
                    SdkEvent::Connection(ConnectionEvent::Reconnecting { .. }) => {
                        client.store_state_snapshot(SdkState::Reconnecting);
                    }
                    _ => {}
                }
            }
        });
    }

    /// 中断连接会话监听器
    fn spawn_terminal_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.clone();
        let mut rx = bus.subscribe_raw();
        spawn_im_background(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(_) => break,
                };
                let terminal_reason = match event {
                    SdkEvent::Connection(ConnectionEvent::KickedOff { reason }) => {
                        Some(format!("kicked_off:{reason}"))
                    }
                    SdkEvent::Connection(ConnectionEvent::TokenExpired { message }) => {
                        Some(format!("token_expired:{message}"))
                    }
                    _ => None,
                };
                let Some(reason) = terminal_reason else {
                    continue;
                };
                let applied = client.terminate_session_if_generation(generation).await;
                if applied {
                    tracing::warn!(session_generation = generation, reason = %reason, "session terminated by terminal connection event");
                }
                break;
            }
        });
    }

    fn spawn_reconnect_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.clone();
        let mut rx = bus.subscribe_raw();
        spawn_im_background(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(_) => break,
                };

                let reason = match event {
                    SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => reason,
                    _ => continue,
                };
                if should_skip_reconnect_for_disconnect_reason(&reason) {
                    continue;
                }

                let (user_id, token, interval_secs, max_attempts) =
                    match client.reconnect_snapshot(generation).await {
                        Some(snapshot) => snapshot,
                        None => break,
                    };

                let mut attempt = 0u32;
                loop {
                    if !client.is_generation_current(generation).await {
                        break;
                    }
                    if let Some(max_attempts) = max_attempts
                        && attempt >= max_attempts
                    {
                        tracing::warn!(
                            session_generation = generation,
                            max_attempts,
                            "SDK reconnect attempts exhausted"
                        );
                        client.mark_current_engine_disconnected(generation).await;
                        bus.publish(SdkEvent::Connection(ConnectionEvent::Disconnected {
                            reason: "reconnect attempts exhausted".to_string(),
                        }));
                        break;
                    }

                    attempt += 1;
                    client.store_state_snapshot(SdkState::Reconnecting);
                    bus.publish(SdkEvent::Connection(ConnectionEvent::Reconnecting {
                        attempt,
                    }));
                    let delay_secs = reconnect_delay_secs(interval_secs, attempt);
                    delay(Duration::from_secs(delay_secs)).await;

                    if client.is_current_transport_connected(generation).await {
                        tracing::debug!(
                            session_generation = generation,
                            attempt,
                            reason = %reason,
                            "skip stale reconnect event because transport is already connected"
                        );
                        break;
                    }

                    match client
                        .reconnect_current_engine(generation, &user_id, &token)
                        .await
                    {
                        Ok(()) => {
                            tracing::info!(
                                session_generation = generation,
                                attempt,
                                "SDK reconnect succeeded"
                            );
                            break;
                        }
                        Err(err) => {
                            tracing::warn!(
                                session_generation = generation,
                                attempt,
                                error = %err,
                                "SDK reconnect failed"
                            );
                        }
                    }
                }
            }
        });
    }

    async fn reconnect_snapshot(
        &self,
        generation: u64,
    ) -> Option<(String, String, u64, Option<u32>)> {
        let g = self.inner.read().await;
        if g.session_generation != generation {
            return None;
        }
        let user_id = g.current_user_id.clone()?;
        let token = g
            .connect_token
            .clone()
            .or_else(|| resolve_connect_token(&user_id, None).ok())?;
        let interval_secs = g
            .sdk_config
            .as_ref()
            .and_then(|c| c.reconnect_interval_secs)
            .unwrap_or(5)
            .max(1);
        let max_attempts = g
            .sdk_config
            .as_ref()
            .and_then(|c| c.max_reconnect_attempts)
            .map(|attempts| attempts.max(1));
        Some((user_id, token, interval_secs, max_attempts))
    }

    async fn is_generation_current(&self, generation: u64) -> bool {
        self.inner.read().await.session_generation == generation
    }

    async fn is_current_transport_connected(&self, generation: u64) -> bool {
        let g = self.inner.read().await;
        if g.session_generation != generation {
            return false;
        }
        match g.engine.as_ref() {
            Some(engine) => engine.transport_connected().await,
            None => false,
        }
    }

    async fn reconnect_current_engine(
        &self,
        generation: u64,
        user_id: &str,
        token: &str,
    ) -> Result<()> {
        let mut engine = {
            let mut g = self.inner.write().await;
            if g.session_generation != generation {
                return Ok(());
            }
            g.engine.take().ok_or_else(Self::not_connected)?
        };

        self.store_state_snapshot(SdkState::Reconnecting);
        let result = engine.reconnect(user_id, token).await;
        let state = engine.state();

        let mut g = self.inner.write().await;
        if g.session_generation == generation {
            g.engine = Some(engine);
            self.store_state_snapshot(state);
        }
        result
    }

    async fn mark_current_engine_disconnected(&self, generation: u64) {
        let engine = {
            let mut g = self.inner.write().await;
            if g.session_generation != generation {
                return;
            }
            g.engine.take()
        };
        let Some(engine) = engine else {
            return;
        };
        engine.mark_transport_disconnected().await;
        self.store_state_snapshot(SdkState::Disconnected);

        let mut g = self.inner.write().await;
        if g.session_generation == generation {
            g.engine = Some(engine);
        }
    }

    async fn terminate_session_if_generation(&self, generation: u64) -> bool {
        let engine = {
            let mut g = self.inner.write().await;
            if g.session_generation != generation {
                return false;
            }
            if g.engine.is_none() && g.current_user_id.is_none() {
                return false;
            }
            g.session_generation = g.session_generation.wrapping_add(1);
            g.current_user_id = None;
            g.connect_token = None;
            g.message_api = None;
            g.media_api = None;
            g.capability_api = None;
            g.presence_api = None;
            g.capability_registry = None;
            g.message_build_api = None;
            g.conversation_api = None;
            g.engine.take()
        };
        self.store_state_snapshot(SdkState::Disconnected);
        if let Some(mut e) = engine
            && let Err(err) = e.disconnect().await
        {
            tracing::warn!(%err, "disconnect after terminal event failed");
        }
        true
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

    /// 获取底层存储提供者（Message/Conversation/Cursor/PendingSend）。
    ///
    /// 通常用于诊断、调试或高级扩展场景。
    pub fn stores(&self) -> Result<StoreProvider> {
        self.with_engine(|e| e.stores().clone())
    }

    pub async fn stores_async(&self) -> Result<StoreProvider> {
        self.with_engine_async(|e| e.stores().clone()).await
    }

    async fn extension_lifecycle_snapshot(
        &self,
        current_user_id: Option<String>,
    ) -> Option<(Arc<ExtensionRuntime>, ExtensionLifecycleContext)> {
        let g = self.inner.read().await;
        let runtime = g.extension_runtime.clone()?;
        if runtime.lifecycle_count() == 0 {
            return None;
        }
        let engine = g.engine.as_ref()?;
        let current_user_id = current_user_id.or_else(|| g.current_user_id.clone());
        Some((
            runtime,
            ExtensionLifecycleContext::new(
                ExtensionContext::from_core(engine.stores().clone(), engine.bus().clone()),
                current_user_id,
            ),
        ))
    }

    async fn notify_extension_login_best_effort(&self, user_id: &str) {
        if let Some((runtime, context)) = self
            .extension_lifecycle_snapshot(Some(user_id.to_string()))
            .await
        {
            if let Err(err) = runtime.notify_login(&context).await {
                tracing::warn!(
                    target = "flare_sdk.extension",
                    user_id = user_id,
                    error = %err,
                    "SDK extension login lifecycle failed"
                );
            }
        }
    }

    async fn notify_extension_connect_best_effort(&self, user_id: &str) {
        if let Some((runtime, context)) = self
            .extension_lifecycle_snapshot(Some(user_id.to_string()))
            .await
        {
            if let Err(err) = runtime.notify_connect(&context).await {
                tracing::warn!(
                    target = "flare_sdk.extension",
                    user_id = user_id,
                    error = %err,
                    "SDK extension connect lifecycle failed"
                );
            }
        }
    }

    async fn notify_extension_disconnect_snapshot(
        lifecycle: &Option<(Arc<ExtensionRuntime>, ExtensionLifecycleContext)>,
    ) {
        if let Some((runtime, context)) = lifecycle {
            if let Err(err) = runtime.notify_disconnect(context).await {
                tracing::warn!(
                    target = "flare_sdk.extension",
                    user_id = context.current_user_id().unwrap_or_default(),
                    error = %err,
                    "SDK extension disconnect lifecycle failed"
                );
            }
        }
    }

    async fn notify_extension_logout_snapshot(
        lifecycle: &Option<(Arc<ExtensionRuntime>, ExtensionLifecycleContext)>,
    ) {
        if let Some((runtime, context)) = lifecycle {
            if let Err(err) = runtime.notify_logout(context).await {
                tracing::warn!(
                    target = "flare_sdk.extension",
                    user_id = context.current_user_id().unwrap_or_default(),
                    error = %err,
                    "SDK extension logout lifecycle failed"
                );
            }
        }
    }

    /// 获取扩展运行时快照。
    pub fn extension_runtime(&self) -> Result<Arc<ExtensionRuntime>> {
        self.read_inner()?
            .extension_runtime
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 构造受控扩展上下文，供业务 SDK 投影资料、会话等核心模型。
    pub async fn extension_context(&self) -> Result<ExtensionContext> {
        self.with_engine_async(|engine| {
            ExtensionContext::from_core(engine.stores().clone(), engine.bus().clone())
        })
        .await
    }

    /// 触发指定会话的增量消息同步（从服务端拉取该会话最新数据）。
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        let runner = self
            .with_engine_async(|e| e.session_sync_runner())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner.request_message_sync(conversation_id).await
    }

    /// 静默拉取会话列表摘要（多端补偿，不阻塞 UI）。
    pub async fn sync_conversation_summaries_silent(&self) -> Result<()> {
        let sync = self
            .with_engine_async(|engine| engine.conversation_summary_sync())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        let user_id = self.current_user_id().await.unwrap_or_default();
        if user_id.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        sync.sync_conversation_summaries(
            &user_id,
            SyncRunContext::silent_multidevice_private_data(),
        )
        .await
    }

    /// 静默拉取会话列表摘要，并返回调用方缺失或版本落后的会话。
    pub async fn sync_conversation_summaries_with_versions(
        &self,
        request: SyncConversationSummariesRequest,
    ) -> Result<SyncConversationSummariesResponse> {
        self.sync_conversation_summaries_silent().await?;
        let conversations = self.conversation_async().await?.list_raw().await?;
        let current_versions = conversations
            .into_iter()
            .map(|conversation| ConversationVersion {
                conversation_id: conversation.conversation_id,
                version: conversation.version,
            });

        Ok(SyncConversationSummariesResponse::from_current_versions(
            &request.known_versions,
            current_versions,
        ))
    }

    /// 按 task id 静默触发 Background 同步任务。
    pub async fn spawn_background_sync_tasks(&self, task_ids: &[&str]) -> Result<()> {
        let (sync_manager, store, bus) = self
            .with_engine_async(|engine| {
                (
                    engine.sync_manager(),
                    engine.stores().clone(),
                    engine.bus().clone(),
                )
            })
            .await?;
        let user_id = self.current_user_id().await.unwrap_or_default();
        if user_id.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        sync_manager.spawn_background_tasks_by_ids(&user_id, task_ids, store, bus);
        Ok(())
    }

    pub async fn sync_conversation_participants(
        &self,
        conversation_id: &str,
        limit: i32,
    ) -> Result<Vec<crate::model::ConversationParticipant>> {
        let runner = self
            .with_engine_async(|e| e.session_sync_runner())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner
            .request_participants_sync(conversation_id, limit)
            .await
    }

    /// 从指定序列号开始拉取会话消息。
    ///
    /// `last_seq` 为客户端已知游标，`limit` 为单次请求上限。
    pub async fn sync_messages(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Result<()> {
        let runner = self
            .with_engine_async(|e| e.session_sync_runner())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner
            .request_message_sync_from_seq(conversation_id, last_seq, limit)
            .await
    }

    /// 将会话读位对齐到 `read_seq`，并同步上报已读状态。
    ///
    /// 内部会同时更新 `message` 与 `conversation` 读态，并发送读回执。
    pub async fn mark_session_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        let conversation = self.conversation_async().await?;
        conversation.mark_read(conversation_id, read_seq).await?;
        let effective_read_seq = conversation
            .get(conversation_id)
            .await?
            .map(|conv| conv.last_read_seq)
            .unwrap_or(read_seq);
        tracing::debug!(
            conversation_id = %conversation_id,
            requested_read_seq = read_seq,
            effective_read_seq = effective_read_seq,
            "mark_session_read resolved effective seq"
        );
        let message = self.message_async().await?;
        message
            .mark_read(conversation_id, effective_read_seq)
            .await?;
        if let Some(runner) = self.with_engine_async(|e| e.session_sync_runner()).await? {
            // 上报“真实已读位点”到服务端。
            // 历史上 read_seq=0 会直接上报 0，但后端并未统一按“全部已读”解释 0，
            // 会导致重登后 last_read_seq 未推进、已读双勾丢失。
            let ack_read_seq = effective_read_seq;
            tracing::debug!(
                conversation_id = %conversation_id,
                requested_read_seq = read_seq,
                effective_read_seq = effective_read_seq,
                ack_read_seq = ack_read_seq,
                "mark_session_read dispatch ack"
            );
            runner.send_read_ack(conversation_id, ack_read_seq).await?;
        }
        Ok(())
    }

    /// 设置会话输入状态（typing/not typing）。
    pub async fn set_conversation_input_state(
        &self,
        conversation_id: &str,
        is_typing: bool,
    ) -> Result<()> {
        self.message_async()
            .await?
            .typing(conversation_id, is_typing)
            .await
    }

    pub async fn message_async(&self) -> Result<MessageApi> {
        let g = self.read_inner_async().await?;
        g.message_api.clone().ok_or_else(Self::not_connected)
    }

    pub async fn conversation_async(&self) -> Result<ConversationApi> {
        let g = self.read_inner_async().await?;
        g.conversation_api
            .as_ref()
            .map(|a| a.as_ref().clone())
            .ok_or_else(Self::not_connected)
    }

    pub async fn get_user_presence(&self, user_id: &str) -> Result<UserPresenceDto> {
        let g = self.read_inner_async().await?;
        let api = g.presence_api.as_ref().ok_or_else(Self::not_connected)?;
        api.get_user_presence(user_id).await
    }

    pub async fn batch_get_user_presence(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::HashMap<String, UserPresenceDto>> {
        let g = self.read_inner_async().await?;
        let api = g.presence_api.as_ref().ok_or_else(Self::not_connected)?;
        api.batch_get_user_presence(user_ids).await
    }

    pub async fn subscribe_user_presence(&self, user_ids: Vec<String>) -> Result<()> {
        let g = self.read_inner_async().await?;
        let api = g.presence_api.as_ref().ok_or_else(Self::not_connected)?;
        api.subscribe_user_presence(user_ids).await
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
mod tests {
    use std::sync::Arc;

    use super::{
        CoreTokenConfig, IMClient, SdkConfigOverlay, SdkState, reconnect_delay_bounds_secs,
        reconnect_delay_secs, should_skip_reconnect_for_disconnect_reason,
    };
    use crate::infrastructure::transport::http::HttpRequestContext;
    use crate::shared::error::ErrorCode;

    #[test]
    fn reconnect_delay_uses_capped_exponential_backoff_with_jitter_window() {
        assert_eq!(reconnect_delay_bounds_secs(5, 1), (4, 6));
        assert_eq!(reconnect_delay_bounds_secs(5, 2), (8, 12));
        assert_eq!(reconnect_delay_bounds_secs(5, 3), (16, 24));
        assert_eq!(reconnect_delay_bounds_secs(5, 4), (24, 30));
        assert_eq!(reconnect_delay_bounds_secs(5, 10), (24, 30));

        for attempt in 1..=10 {
            let (min, max) = reconnect_delay_bounds_secs(5, attempt);
            let actual = reconnect_delay_secs(5, attempt);
            assert!(
                (min..=max).contains(&actual),
                "attempt {attempt} delay {actual} outside {min}..={max}"
            );
        }
    }

    #[test]
    fn local_client_disconnect_reasons_do_not_schedule_reconnect() {
        assert!(should_skip_reconnect_for_disconnect_reason(
            "Client disconnected"
        ));
        assert!(should_skip_reconnect_for_disconnect_reason(
            "Closed by client"
        ));
        assert!(should_skip_reconnect_for_disconnect_reason(
            " transport: Client disconnected "
        ));
        assert!(should_skip_reconnect_for_disconnect_reason(
            "websocket Closed by client"
        ));
    }

    #[test]
    fn generate_core_token_requires_explicit_signing_config() {
        let err = IMClient::generate_core_token(CoreTokenConfig {
            secret: String::new(),
            issuer: "flare-im-core".to_string(),
            user_id: "alice".to_string(),
            ttl_secs: 3600,
            device_id: None,
            tenant_id: None,
        })
        .expect_err("production build must not mint unsigned or default-signed tokens");

        assert_eq!(
            err.code(),
            Some(crate::shared::error::ErrorCode::ConfigurationError)
        );
    }

    #[tokio::test]
    async fn uninit_clears_init_configuration() {
        let client = IMClient::new();
        let data_root =
            std::env::temp_dir().join(format!("flare-im-uninit-test-{}", std::process::id()));
        client
            .init(
                Some("dev".to_string()),
                Some(SdkConfigOverlay {
                    data_url: Some(format!("file://{}", data_root.display())),
                    ws_url: Some("ws://localhost:60051".to_string()),
                    ..SdkConfigOverlay::default()
                }),
            )
            .await
            .expect("init sdk");

        assert!(client.data_root().await.is_some());
        client.uninit().await.expect("uninit sdk");

        let (environment, sdk_config) = client.config_snapshot().await;
        assert!(environment.is_none());
        assert!(sdk_config.is_none());
        assert!(client.data_root().await.is_none());
        assert!(!client.session_active_sync());
        let _ = tokio::fs::remove_dir_all(data_root).await;
    }

    #[tokio::test]
    async fn session_active_sync_is_false_for_prepared_but_disconnected_user() {
        let client = IMClient::new();
        {
            let mut inner = client.inner.write().await;
            inner.current_user_id = Some("alice".to_string());
        }

        assert!(!client.session_active_sync());
    }

    #[tokio::test]
    async fn session_active_sync_requires_user_and_active_state() {
        let client = IMClient::new();
        client.store_state_snapshot(SdkState::Ready);
        assert!(!client.session_active_sync());

        {
            let mut inner = client.inner.write().await;
            inner.current_user_id = Some("alice".to_string());
        }
        assert!(client.session_active_sync());
    }

    #[tokio::test]
    async fn update_access_token_replaces_existing_gateway_bearer() {
        let context = Arc::new(HttpRequestContext::new());
        context
            .set_gateway_context(
                "old-gateway-token".to_string(),
                "tenant-a".to_string(),
                "alice".to_string(),
                None,
            )
            .await;
        let client = IMClient::new();
        {
            let mut inner = client.inner.write().await;
            inner.current_user_id = Some("alice".to_string());
            inner.connect_token = Some("old-im-token".to_string());
            inner.http_request_context = Some(context.clone());
        }
        client.store_state_snapshot(SdkState::Ready);

        client
            .update_access_token("new-gateway-token", Some("tenant-b"))
            .await
            .expect("update token");

        let headers = context.build_headers().await;
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer new-gateway-token")
        );
        assert_eq!(
            headers.get("x-tenant-id").map(String::as_str),
            Some("tenant-b")
        );
        assert_eq!(headers.get("x-user-id").map(String::as_str), Some("alice"));
    }

    #[tokio::test]
    async fn update_access_token_rejects_prepared_but_disconnected_session() {
        let context = Arc::new(HttpRequestContext::new());
        context
            .set_gateway_context(
                "old-gateway-token".to_string(),
                "tenant-a".to_string(),
                "alice".to_string(),
                None,
            )
            .await;
        let client = IMClient::new();
        {
            let mut inner = client.inner.write().await;
            inner.current_user_id = Some("alice".to_string());
            inner.connect_token = Some("old-im-token".to_string());
            inner.http_request_context = Some(context.clone());
        }

        let err = client
            .update_access_token("new-gateway-token", Some("tenant-b"))
            .await
            .expect_err("prepared but disconnected session must not refresh gateway auth");
        assert_eq!(err.code(), Some(ErrorCode::NotConnected));

        let headers = context.build_headers().await;
        assert_eq!(
            headers.get("Authorization").map(String::as_str),
            Some("Bearer old-gateway-token")
        );
        assert_eq!(
            headers.get("x-tenant-id").map(String::as_str),
            Some("tenant-a")
        );
    }

    #[tokio::test]
    async fn disconnect_clears_shared_http_auth_context() {
        let context = Arc::new(HttpRequestContext::new());
        context.set_auth_context("im-token".to_string(), None).await;
        context
            .set_gateway_context(
                "gateway-token".to_string(),
                "tenant-a".to_string(),
                "alice".to_string(),
                None,
            )
            .await;
        let client = IMClient::new();
        {
            let mut inner = client.inner.write().await;
            inner.current_user_id = Some("alice".to_string());
            inner.connect_token = Some("im-token".to_string());
            inner.http_request_context = Some(context.clone());
        }

        client.disconnect().await.expect("disconnect");

        let headers = context.build_headers().await;
        assert_eq!(headers.get("Authorization"), None);
        assert_eq!(headers.get("x-user-id"), None);
        assert_eq!(headers.get("x-tenant-id").map(String::as_str), Some("0"));
    }
}
