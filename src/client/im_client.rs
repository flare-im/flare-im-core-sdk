//! [`IMClient`]：初始化、登录、登出与消息/会话 API（单一入口）。
//!
//! 内部为 [`tokio::sync::RwLock`]：同步读路径使用 `try_read`（禁止 `blocking_read`），避免在 `#[tokio::main]` 或异步任务中 panic。

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::FlareError;
use crate::Result;
use crate::capability::SdkCapabilityRegistry;
use crate::client::api::{
    CapabilityApi, CapabilityDispatchResult, ConversationApi, MediaApi, MessageApi,
    MessageBuildApi, PresenceApi, UserCapabilityGrantDto, UserPresenceDto,
};
use crate::client::builder::IMClientBuilder;
use crate::client::lifecycle::{
    LoginDbKind, SdkConfigOverlay, default_ws_url, merge_sdk_config, parse_data_url_to_path,
    resolve_connect_token,
};
use crate::core::{SdkEngine, SdkState};
use crate::error::ErrorCode;
use crate::event::{ConnectionEvent, EventBus, MessageEvent, SdkEvent};
use crate::model::message::MessageLocalState;
use crate::store::StoreProvider;
use crate::transport::http::HttpRequestContext;
use crate::util::generate_test_token as util_generate_test_token;
use flare_proto::common::CallSignalEvent;
use flare_proto::common::MessageStatus;
use serde_json::Value;
use std::time::Duration;

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
    pub message_build_api: Option<Arc<MessageBuildApi>>,
    pub conversation_api: Option<Arc<ConversationApi>>,
    pub http_request_context: Option<Arc<HttpRequestContext>>,
    pub session_generation: u64,
}

/// 唯一 SDK 句柄：[`Self::init`] → [`Self::login`]，或 [`Self::builder`] → [`Self::connect`]。
#[derive(Clone)]
pub struct IMClient {
    pub(crate) inner: Arc<RwLock<IMClientInner>>,
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
        }
    }

    /// 返回链式构建器，用于自定义存储、编解码器与中间件后再构建 [`IMClient`]。
    pub fn builder() -> IMClientBuilder {
        IMClientBuilder::new()
    }

    pub(crate) fn from_inner(inner: IMClientInner) -> Self {
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub(crate) fn into_inner(self) -> IMClientInner {
        match Arc::try_unwrap(self.inner) {
            Ok(rw) => rw.into_inner(),
            Err(_) => panic!("IMClient must be uniquely owned"),
        }
    }

    fn not_connected() -> FlareError {
        FlareError::localized(ErrorCode::NotConnected, "SDK not connected")
    }

    fn lock_busy() -> FlareError {
        FlareError::localized(ErrorCode::InternalError, "IMClient lock busy")
    }

    /// 同步 API 使用的读锁：在 Tokio worker 上 **禁止** `blocking_read`，必须用 `try_read`。
    pub(crate) fn read_inner(&self) -> Result<tokio::sync::RwLockReadGuard<'_, IMClientInner>> {
        self.inner.try_read().map_err(|_| Self::lock_busy())
    }

    pub(crate) fn with_engine<R>(&self, f: impl FnOnce(&SdkEngine) -> R) -> Result<R> {
        let g = self.read_inner()?;
        let e = g.engine.as_ref().ok_or_else(Self::not_connected)?;
        Ok(f(e))
    }

    /// 初始化运行环境与 SDK 配置快照。
    ///
    /// - 仅更新本地配置，不建连；
    /// - 若传入 `sdk_config.data_url`，会解析并创建数据目录；
    /// - 后续 [`Self::login`] 会基于该配置构建实际存储与连接参数。
    pub async fn init(
        &self,
        environment: Option<String>,
        sdk_config: Option<SdkConfigOverlay>,
    ) -> Result<()> {
        let mut g = self.inner.write().await;
        g.environment = environment;
        g.data_root = None;
        if let Some(ref cfg) = sdk_config {
            if let Some(ref url) = cfg.data_url {
                let path = parse_data_url_to_path(url)?;
                std::fs::create_dir_all(&path).map_err(|e| {
                    FlareError::localized(
                        ErrorCode::InvalidParameter,
                        format!("data_url create_dir_all failed: {}", e),
                    )
                })?;
                g.data_root = Some(path);
            }
        }
        g.sdk_config = sdk_config;
        Ok(())
    }

    /// 返回当前配置的 SDK 数据根目录（来自 `init(sdkConfig.dataUrl)`）。
    pub async fn data_root(&self) -> Option<PathBuf> {
        self.inner.read().await.data_root.clone()
    }

    /// 基于数据根目录解析一个子路径并确保父目录存在。
    ///
    /// 常用于上层保存附件、缓存或导出文件。若尚未 `init` 或未设置 `dataUrl` 会返回错误。
    pub async fn resolve_data_subpath(
        &self,
        relative: impl AsRef<std::path::Path>,
    ) -> Result<PathBuf> {
        let root = self.data_root().await.ok_or_else(|| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                "init: set sdkConfig.dataUrl before resolving subpaths",
            )
        })?;
        let p = root.join(relative.as_ref());
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
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

    /// 与 [`Self::state`] / 传输层 `Ready` 不同：未登录时引擎可能仍存在，此时本方法为 `false`。
    pub fn session_active_sync(&self) -> bool {
        self.inner
            .try_read()
            .map(|g| g.current_user_id.as_ref().is_some_and(|s| !s.is_empty()))
            .unwrap_or(false)
    }

    /// 返回当前登录用户 ID；未登录时返回 `None`。
    pub async fn current_user_id(&self) -> Option<String> {
        self.inner.read().await.current_user_id.clone()
    }

    /// 生成开发/测试用 JWT token。
    ///
    /// 当 `secret`/`issuer` 为空时会使用内置默认值，仅适用于开发环境。
    pub fn generate_test_token(
        secret: &str,
        issuer: &str,
        user_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<String> {
        let s = if secret.is_empty() {
            "insecure-secret"
        } else {
            secret
        };
        let i = if issuer.is_empty() {
            "flare-im-core"
        } else {
            issuer
        };
        util_generate_test_token(s, i, user_id, 3600, None, tenant_id)
    }

    /// 主动退出登录并清空 SDK 会话上下文。
    ///
    /// 该操作会断开连接、推进会话代际并清理 `message/conversation` API 句柄；
    /// 调用后需重新 `login` 或 `connect` 才可继续收发消息。
    pub async fn logout(&self) -> Result<()> {
        let (engine, presence_api, http_request_context) = {
            let mut g = self.inner.write().await;
            let presence_api = g.presence_api.clone();
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
            (engine, presence_api, http_request_context)
        };
        if let Some(api) = presence_api.as_ref()
            && let Err(err) = api.logout_current_device_presence().await
        {
            tracing::warn!(%err, "active presence logout failed; falling back to transport disconnect");
        }
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(String::new(), None).await;
        }
        if let Some(mut e) = engine {
            e.disconnect().await?;
        }
        Ok(())
    }

    /// 登录入口：按用户初始化存储、建立连接并切换为新会话。
    ///
    /// - 会先执行一次 [`Self::logout`]，确保会话隔离；
    /// - `before_connect` 可在建连前注册事件监听；
    /// - 被踢下线 / token 过期将由内部 watcher 自动终止会话（等价登出）。
    pub async fn login<F>(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
        db: LoginDbKind,
        before_connect: F,
    ) -> Result<()>
    where
        F: FnOnce(crate::event::EventBus, Arc<dyn crate::domain::MessageStore>) + Send + 'static,
    {
        self.logout().await?;
        let snap = {
            let g = self.inner.read().await;
            (
                g.environment.clone(),
                g.sdk_config.clone(),
                g.data_root.clone(),
            )
        };
        let stores = match db {
            #[cfg(feature = "lifecycle-sqlite")]
            LoginDbKind::Sqlite => {
                let base = snap.2.clone().ok_or_else(|| {
                    FlareError::localized(
                        ErrorCode::InvalidParameter,
                        "init: set sdkConfig.dataUrl before SQLite login",
                    )
                })?;
                crate::util::sqlite_store::open_sqlite_store_for_user(&base, user_id).await?
            }
            LoginDbKind::IndexedDb(stores) => stores,
        };
        let ws_url = default_ws_url(snap.1.as_ref());
        let config = merge_sdk_config(&ws_url, snap.1.as_ref());
        let child = IMClientBuilder::new().config(config).stores(stores).build();
        let bus = child.bus()?.clone();
        let msg_store = child.stores()?.messages.clone();
        before_connect(bus, msg_store);
        child
            .connect_internal(user_id, explicit_token, false)
            .await?;
        child.reset_pending_queue_on_login().await?;
        let mut inner = child.into_inner();
        let next_generation = {
            let g = self.inner.read().await;
            g.session_generation.wrapping_add(1)
        };
        inner.environment = snap.0;
        inner.sdk_config = snap.1;
        inner.data_root = snap.2;
        inner.current_user_id = Some(user_id.to_string());
        inner.connect_token = explicit_token
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| resolve_connect_token(user_id, None).ok());
        inner.session_generation = next_generation;
        let login_bus = inner.engine.as_ref().map(|e| e.bus().clone());
        *self.inner.write().await = inner;
        if let Some(bus) = login_bus {
            self.spawn_terminal_session_watcher(next_generation, bus.clone());
            self.spawn_reconnect_session_watcher(next_generation, bus);
        }
        Ok(())
    }

    /// 使用当前客户端已装配引擎执行连接。
    ///
    /// 适用于 builder 路径。若引擎不存在（未 build/login）将返回错误。
    pub async fn connect(&self, user_id: &str, explicit_token: Option<&str>) -> Result<()> {
        self.connect_internal(user_id, explicit_token, true).await
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
        if let Err(error) = e.connect(user_id, &token).await {
            let mut g = self.inner.write().await;
            if g.engine.is_none() {
                g.engine = Some(e);
            }
            return Err(error);
        }
        let bus = e.bus().clone();
        let mut g = self.inner.write().await;
        g.engine = Some(e);
        g.current_user_id = Some(user_id.to_string());
        g.connect_token = Some(token.clone());
        g.session_generation = g.session_generation.wrapping_add(1);
        let current_generation = g.session_generation;
        drop(g);
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(token.clone(), None).await;
        }
        if install_watcher {
            self.spawn_terminal_session_watcher(current_generation, bus.clone());
            self.spawn_reconnect_session_watcher(current_generation, bus);
        }
        Ok(())
    }

    /// 主动断开连接并清理当前会话上下文（语义上等价于轻量登出）。
    pub async fn disconnect(&self) -> Result<()> {
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
        if let Some(context) = http_request_context.as_ref() {
            context.set_auth_context(String::new(), None).await;
        }
        if let Some(mut e) = engine {
            e.disconnect().await?;
        }
        Ok(())
    }

    /// 读取 SDK 当前连接状态快照（FSM 驱动）。
    ///
    /// 若锁竞争或引擎缺失，返回 `Disconnected`。
    pub fn state(&self) -> SdkState {
        match self.inner.try_read() {
            Ok(g) => g
                .engine
                .as_ref()
                .map(|e| e.state())
                .unwrap_or(SdkState::Disconnected),
            Err(_) => SdkState::Disconnected,
        }
    }

    /// 获取消息 API 门面；未连接时返回 `NotConnected`。
    pub fn message(&self) -> Result<MessageApi> {
        self.read_inner()?
            .message_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取消息构建 API（负责组装 `IMMessage`）；未连接时返回 `NotConnected`。
    pub fn message_build(&self) -> Result<Arc<MessageBuildApi>> {
        self.read_inner()?
            .message_build_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取会话 API 门面；未连接时返回 `NotConnected`。
    pub fn conversation(&self) -> Result<ConversationApi> {
        self.read_inner()?
            .conversation_api
            .as_ref()
            .map(|a| a.as_ref().clone())
            .ok_or_else(Self::not_connected)
    }

    /// 获取媒体 API 门面（上传/删除）。
    pub fn media(&self) -> Result<Arc<MediaApi>> {
        self.read_inner()?
            .media_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取能力插件 API（付费模块入口，包含 RTC/SFU 能力）。
    pub fn capability(&self) -> Result<Arc<CapabilityApi>> {
        self.read_inner()?
            .capability_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取用户在线状态 API。
    pub fn presence(&self) -> Result<Arc<PresenceApi>> {
        self.read_inner()?
            .presence_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    /// 获取 SDK 能力插件注册表（支持多付费插件扩展）。
    pub fn capability_registry(&self) -> Result<Arc<SdkCapabilityRegistry>> {
        self.read_inner()?
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

    /// 上行发送通话信令（`EVENT_CALL_SIGNAL`，经 WebSocket `PacketSender::send_event`）。
    pub async fn send_call_signal(
        &self,
        conversation_id: &str,
        call: CallSignalEvent,
    ) -> Result<()> {
        let wire =
            crate::capability::call_event::event_call_signal_uplink(conversation_id, 0, call);
        let sender = self.with_engine(|e| e.sender().clone())?;
        sender.send_event(&wire, Duration::from_secs(30)).await
    }

    /// 获取 SDK 事件总线（用于原始事件订阅或桥接到宿主事件系统）。
    pub fn bus(&self) -> Result<EventBus> {
        self.with_engine(|e| e.bus().clone())
    }
    /// 中断连接会话监听器
    fn spawn_terminal_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.clone();
        let mut rx = bus.subscribe_raw();
        tokio::spawn(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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
        tokio::spawn(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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
                    bus.publish(SdkEvent::Connection(ConnectionEvent::Reconnecting {
                        attempt,
                    }));
                    let delay_secs = reconnect_delay_secs(interval_secs, attempt);
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;

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

        let result = engine.reconnect(user_id, token).await;

        let mut g = self.inner.write().await;
        if g.session_generation == generation {
            g.engine = Some(engine);
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
        if let Some(mut e) = engine {
            if let Err(err) = e.disconnect().await {
                tracing::warn!(%err, "disconnect after terminal event failed");
            }
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
        let dropped_client_ids = if let Some(queue) = self.with_engine(|e| e.reliable_queue())? {
            // 由队列 actor 原子处理 in_flight + pending，避免与后台 tick 竞态。
            should_publish_failed = false;
            queue.reset_pending_on_login().await?
        } else {
            // 兜底分支：无可靠队列实现时，沿用仓储清理逻辑。
            let current_user_id = self.current_user_id().await.unwrap_or_default();
            if current_user_id.trim().is_empty() {
                return Ok(());
            }
            let stores = self.stores()?;
            let Some((pending_reader, pending_writer)) = stores.pending_sends() else {
                return Ok(());
            };
            let pending_entries = pending_reader.list().await?;
            if pending_entries.is_empty() {
                return Ok(());
            }
            let mut dropped_client_ids = Vec::with_capacity(pending_entries.len());
            for entry in pending_entries {
                let _ = pending_writer.pop(&entry.client_msg_id).await?;
                if let Some(mut local) = stores
                    .messages
                    .get_by_client_msg_id(&entry.client_msg_id)
                    .await?
                {
                    local.server_id = local.client_msg_id.clone();
                    local.local_state = MessageLocalState {
                        sending: false,
                        failed: true,
                        is_local: true,
                        sort_ts: local.local_state.sort_ts,
                    };
                    local.status = MessageStatus::Failed as i32;
                    stores.messages.save_batch(&[local]).await?;
                }
                dropped_client_ids.push(entry.client_msg_id);
            }
            dropped_client_ids
        };

        if dropped_client_ids.is_empty() {
            return Ok(());
        }

        if !should_publish_failed {
            return Ok(());
        }

        let bus = self.bus()?;
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

    /// 触发指定会话的增量消息同步（从服务端拉取该会话最新数据）。
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        let runner = self
            .with_engine(|e| e.session_sync_runner())?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner.request_message_sync(conversation_id).await
    }

    pub async fn sync_conversation_participants(
        &self,
        conversation_id: &str,
        limit: i32,
    ) -> Result<Vec<crate::model::ConversationParticipant>> {
        let runner = self
            .with_engine(|e| e.session_sync_runner())?
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
            .with_engine(|e| e.session_sync_runner())?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner
            .request_message_sync_from_seq(conversation_id, last_seq, limit)
            .await
    }

    /// 将会话读位对齐到 `read_seq`，并同步上报已读状态。
    ///
    /// 内部会同时更新 `message` 与 `conversation` 读态，并发送读回执。
    pub async fn mark_session_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        let c = self.conversation()?;
        c.mark_read(conversation_id, read_seq).await?;
        let effective_read_seq = c
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
        let m = self.message()?;
        m.mark_read(conversation_id, effective_read_seq).await?;
        if let Some(runner) = self.with_engine(|e| e.session_sync_runner())? {
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
        self.message()?.typing(conversation_id, is_typing).await
    }

    pub async fn get_user_presence(&self, user_id: &str) -> Result<UserPresenceDto> {
        self.presence()?.get_user_presence(user_id).await
    }

    pub async fn batch_get_user_presence(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::HashMap<String, UserPresenceDto>> {
        self.presence()?.batch_get_user_presence(user_ids).await
    }

    pub async fn subscribe_user_presence(&self, user_ids: Vec<String>) -> Result<()> {
        self.presence()?.subscribe_user_presence(user_ids).await
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
    let shift = attempt.saturating_sub(1).min(4);
    base_interval_secs
        .saturating_mul(1_u64 << shift)
        .clamp(1, 30)
}

impl Default for IMClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{reconnect_delay_secs, should_skip_reconnect_for_disconnect_reason};

    #[test]
    fn reconnect_delay_uses_capped_exponential_backoff() {
        assert_eq!(reconnect_delay_secs(5, 1), 5);
        assert_eq!(reconnect_delay_secs(5, 2), 10);
        assert_eq!(reconnect_delay_secs(5, 3), 20);
        assert_eq!(reconnect_delay_secs(5, 4), 30);
        assert_eq!(reconnect_delay_secs(5, 10), 30);
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
}
