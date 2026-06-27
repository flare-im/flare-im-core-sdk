use std::sync::Arc;
use std::time::Duration;

use flare_core::common::{HeartbeatAppState, HeartbeatConfig};

use crate::FlareError;
use crate::client::builder::IMClientBuilder;
use crate::client::connected_apis::ConnectedApis;
use crate::client::lifecycle::{
    LoginDbKind, default_ws_url, merge_sdk_config, resolve_connect_token, resolve_sdk_data_root,
};
use crate::kernel::SdkState;
use crate::shared::error::{ErrorCode, Result};
use crate::shared::util::{
    CoreTokenConfig, generate_core_token as util_generate_core_token, timeout,
};

use super::{IMClient, NetworkChangeEvent, spawn_im_background};

impl IMClient {
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
        self.store_heartbeat_app_state_snapshot(state);
        let g = self.read_inner_async().await?;
        if let Some(engine) = g.engine.as_ref() {
            engine.set_heartbeat_app_state(state).await?;
        }
        if matches!(state, HeartbeatAppState::Foreground) {
            let client = self.clone();
            let generation = self.load_session_generation_snapshot();
            spawn_im_background(async move {
                if client.is_generation_current(generation).await {
                    let _ = client.sync_foreground_convergence_silent().await;
                }
            });
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

    /// 平台网络状态变化通知。网络恢复或 Wi-Fi/蜂窝切换时，SDK 会主动重建长连接，
    /// 让 QUIC/WS 竞速和登录态恢复立即发生，而不是等待心跳超时。
    #[tracing::instrument(skip(self, event), fields(available = event.available, interface = event.interface.as_deref().unwrap_or("unknown")))]
    pub async fn notify_network_change(&self, event: NetworkChangeEvent) -> Result<bool> {
        if !event.available {
            tracing::info!(
                interface = event.interface.as_deref().unwrap_or("unknown"),
                reason = event.reason.as_deref().unwrap_or("network_unavailable"),
                "network unavailable; wait for recovery before reconnect"
            );
            return Ok(false);
        }

        let generation = self.load_session_generation_snapshot();
        let Some((user_id, token, _, _)) = self.reconnect_snapshot(generation).await else {
            return Ok(false);
        };
        if !self.try_begin_network_reconnect() {
            tracing::info!(
                session_generation = generation,
                interface = event.interface.as_deref().unwrap_or("unknown"),
                reason = event.reason.as_deref().unwrap_or("network_change"),
                "network change reconnect already in flight; coalescing event"
            );
            return Ok(false);
        }
        tracing::info!(
            session_generation = generation,
            interface = event.interface.as_deref().unwrap_or("unknown"),
            expensive = event.expensive,
            metered = event.metered,
            reason = event.reason.as_deref().unwrap_or("network_change"),
            "network change reported; proactively reconnecting SDK session"
        );
        let reconnect_result = self
            .reconnect_current_engine(generation, &user_id, &token)
            .await;
        self.finish_network_reconnect();
        reconnect_result?;
        Ok(true)
    }

    /// 与 [`Self::state`] / 传输层 `Ready` 不同：未登录时引擎可能仍存在，此时本方法为 `false`。
    pub fn session_active_sync(&self) -> bool {
        self.active_connected_apis_snapshot().is_some()
    }

    /// 返回当前登录用户 ID；未登录时返回 `None`。
    pub async fn current_user_id(&self) -> Option<String> {
        self.inner.read().await.current_user_id.clone()
    }

    /// 会话代际：登录/重连/登出时递增，供 Tauri `SdkState` 判断 API 快照是否过期。
    pub async fn session_generation(&self) -> u64 {
        self.load_session_generation_snapshot()
    }

    /// 同步读取当前会话代际快照，供绑定层命中缓存时避开 async 锁。
    pub fn session_generation_snapshot(&self) -> u64 {
        self.load_session_generation_snapshot()
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
        self.clear_session_snapshot();
        let lifecycle = self.extension_lifecycle_snapshot(None).await;
        let (engine, presence_api, presence_user_id, http_request_context) = {
            let mut g = self.inner.write().await;
            let presence_api = g.presence_api.clone();
            let presence_user_id = g.current_user_id.clone();
            g.session_generation = g.session_generation.wrapping_add(1);
            self.store_session_generation_snapshot(g.session_generation);
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
        self.clear_session_snapshot();
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
    pub(super) async fn logout_for_login(&self) -> Result<()> {
        const PRESENCE_LOGOUT_TIMEOUT: Duration = Duration::from_secs(2);
        const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(3);

        self.clear_session_snapshot();
        let lifecycle = self.extension_lifecycle_snapshot(None).await;
        let (engine, presence_api, presence_user_id, http_request_context) = {
            let mut g = self.inner.write().await;
            let presence_api = g.presence_api.clone();
            let presence_user_id = g.current_user_id.clone();
            g.session_generation = g.session_generation.wrapping_add(1);
            self.store_session_generation_snapshot(g.session_generation);
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
        self.clear_session_snapshot();
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
    ///
    /// 准备登录会话：打开 per-user 本地库、装配引擎，但**不连网**。
    ///
    /// 配合 [`Self::connect`] 实现「初始化前置、登录只做网络」：App 启动即可对
    /// 「上次登录用户」调用 `prepare`，把开库 / 迁移 / 建引擎 / 待发队列恢复等本地重活
    /// 移出登录关键路径；待拿到 token 再 [`Self::connect`]，登录仅剩连接 + 首次同步。
    ///
    /// 幂等：对任意 user 重复调用都会先清理旧会话、丢弃未连接引擎，再按新 user 重建本地栈。
    #[tracing::instrument(skip(self, db), fields(user_id = %user_id))]
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
            #[cfg(feature = "lifecycle-sqlite")]
            LoginDbKind::EncryptedSqlite {
                key_store,
                key_namespace,
                tenant_id,
                key_name,
            } => {
                let namespace = key_namespace.as_deref().unwrap_or("flare-im-core-sdk");
                let tenant_id = tenant_id
                    .as_deref()
                    .or_else(|| snap.1.as_ref().and_then(|cfg| cfg.tenant_id.as_deref()))
                    .unwrap_or(crate::shared::util::DEFAULT_TENANT_ID);
                crate::shared::util::sqlite_store::open_sqlite_store_for_user_with_secure_key_store(
                    &data_root,
                    user_id,
                    namespace,
                    tenant_id,
                    key_name.as_deref(),
                    key_store.as_ref(),
                )
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
        let session_generation = inner.session_generation;
        *self.inner.write().await = inner;
        self.store_session_generation_snapshot(session_generation);
        self.repair_local_conversation_identities_on_login(user_id)
            .await?;
        tokio::task::yield_now().await;
        self.reset_pending_queue_on_login().await?;
        self.notify_extension_login_best_effort(user_id).await;
        Ok(())
    }

    /// 一步登录：[`Self::prepare`] + [`Self::connect`] 的组合，供不做预热的调用方使用。
    ///
    /// `before_connect` 在 `prepare` 之后、建连之前回调，可在此注册事件监听
    /// （预热路径下等价于在 `prepare` 与 `connect` 之间订阅事件）。
    #[tracing::instrument(skip(self, explicit_token, db, before_connect), fields(user_id = %user_id))]
    pub async fn login<F>(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
        db: LoginDbKind,
        before_connect: F,
    ) -> Result<ConnectedApis>
    where
        F: FnOnce(crate::kernel::event::EventBus, Arc<dyn crate::domain::MessageStore>)
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
    #[tracing::instrument(skip(self, explicit_token), fields(user_id = %user_id))]
    pub async fn connect(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
    ) -> Result<ConnectedApis> {
        self.connect_internal(user_id, explicit_token, true).await?;
        self.connected_apis().await
    }

    #[tracing::instrument(skip(self, explicit_token), fields(user_id = %user_id, install_watcher))]
    pub(super) async fn connect_internal(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
        install_watcher: bool,
    ) -> Result<()> {
        let token = resolve_connect_token(user_id, explicit_token)?;
        self.clear_session_snapshot();
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
        self.store_session_generation_snapshot(g.session_generation);
        let current_generation = g.session_generation;
        let tenant_id = Self::resolve_tenant_id(&g);
        let apis = Self::connected_apis_from_inner(&g)?;
        drop(g);
        self.store_connected_apis_snapshot(apis);
        if let Some(context) = http_request_context.as_ref() {
            // Avoid reusing a stale Social Gateway token after a fresh IM login.
            // Media access resolution can use the IM token fallback until a new
            // gateway session is explicitly applied.
            context.clear_gateway_context().await;
            context.set_auth_context(token.clone(), None).await;
            // IM login 期间 Background 社交同步仍走共享 HTTP，须保留 user/tenant。
            context.ensure_identity(user_id, &tenant_id).await;
        }
        self.notify_extension_connect_best_effort(user_id).await;
        if install_watcher {
            self.spawn_state_snapshot_watcher(current_generation, bus.clone());
            self.spawn_terminal_session_watcher(current_generation, bus.clone());
            self.spawn_reconnect_session_watcher(current_generation, bus);
            self.spawn_foreground_sync_worker(current_generation);
        }
        Ok(())
    }

    /// 主动断开连接并清理当前会话上下文（语义上等价于轻量登出）。
    pub async fn disconnect(&self) -> Result<()> {
        self.clear_session_snapshot();
        let lifecycle = self.extension_lifecycle_snapshot(None).await;
        let (engine, http_request_context) = {
            let mut g = self.inner.write().await;
            g.session_generation = g.session_generation.wrapping_add(1);
            self.store_session_generation_snapshot(g.session_generation);
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
        self.clear_session_snapshot();
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
}
