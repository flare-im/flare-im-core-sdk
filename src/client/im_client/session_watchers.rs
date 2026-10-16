use std::time::Duration;

use crate::FlareError;
use crate::client::token_provider::{GatewayTokenProvider, jwt_exp_secs};
use crate::kernel::event::{ConnectionEvent, EventBus, SdkEvent, SdkEventKind};
use crate::kernel::{SdkState, SyncRunContext};
use crate::shared::error::{ErrorCode, Result, TENANT_UNAVAILABLE, is_tenant_unavailable};
use crate::shared::util::{delay, now_unix_secs};

use super::{
    IMClient, reconnect_delay, should_skip_reconnect_for_disconnect_reason, spawn_im_background,
};

/// 第 `attempt` 次重连前的等待。`after_server_shutdown`：断开原因是服务端停机，且这是停机后的
/// 第一次尝试——在 1–5 s 之间随机等一段（flare-core 的停机约定），而不是按配置的退避：停机是
/// 计划内的，所有客户端同一刻重连会把刚起来的实例冲垮。之后的尝试照常按退避（封顶 30 s、±20% 抖动，
/// 按毫秒取值）。
pub(super) fn reconnect_wait(
    interval_secs: u64,
    attempt: u32,
    after_server_shutdown: bool,
) -> Duration {
    if after_server_shutdown {
        flare_core::common::server_shutdown_reconnect_delay()
    } else {
        reconnect_delay(interval_secs, attempt)
    }
}

/// 会话在一次重连期间（或开始之前）已经结束：这次重连的结果属于已结束的会话。
pub(super) fn session_ended_during_reconnect() -> FlareError {
    FlareError::localized(
        ErrorCode::NotConnected,
        "session ended before the reconnect completed",
    )
}

impl IMClient {
    /// 会话期间按连接事件维护状态快照。会话结束（登出、断开、重新登录、终止会话、客户端销毁）
    /// 时立即收手；此前它要等到下一条连接事件才发现世代已变，一直挂着。
    pub(super) fn spawn_state_snapshot_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.downgrade();
        let session_end = self.session_end_watch(generation);
        // 只消费连接事件——按 Kind 过滤使总线不再为本订阅者深克隆消息批等热事件。
        let mut rx = bus.subscribe_filter(SdkEventKind::Connection);
        spawn_im_background(async move {
            loop {
                let event = tokio::select! {
                    event = rx.recv() => match event {
                        Ok(ev) => ev,
                        Err(_) => break,
                    },
                    () = session_end.ended() => break,
                };
                let Some(client) = client.upgrade() else {
                    break;
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
            tracing::debug!(
                session_generation = generation,
                "SDK state snapshot watcher stopped"
            );
        });
    }

    /// 中断连接会话监听器
    pub(super) fn spawn_terminal_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.downgrade();
        let session_end = self.session_end_watch(generation);
        let mut rx = bus.subscribe_filter(SdkEventKind::Connection);
        spawn_im_background(async move {
            loop {
                let event = tokio::select! {
                    event = rx.recv() => match event {
                        Ok(ev) => ev,
                        Err(_) => break,
                    },
                    () = session_end.ended() => break,
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
                let Some(client) = client.upgrade() else {
                    break;
                };
                // SDK 托管 token：过期不是终局，先刷新再重连；刷新失败才按终局处理。
                if reason.starts_with("token_expired:")
                    && let Some(provider) = client.gateway_token_provider().await
                    && client
                        .refresh_token_and_reconnect(generation, &provider)
                        .await
                {
                    tracing::info!(
                        session_generation = generation,
                        "token expired: refreshed via gateway and reconnected"
                    );
                    continue;
                }
                let applied = client.terminate_session_if_generation(generation).await;
                if applied {
                    tracing::warn!(session_generation = generation, reason = %reason, "session terminated by terminal connection event");
                }
                break;
            }
            tracing::debug!(
                session_generation = generation,
                "SDK terminal session watcher stopped"
            );
        });
    }

    /// 会话期间的重连监视器。会话结束（登出、断开、重新登录、终止会话、客户端销毁）时立即
    /// 收手：在等断开事件或在退避中都会被唤醒；正在进行的那次重连照常收尾，但不再记成功、
    /// 不再继续下一次。
    pub(super) fn spawn_reconnect_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.downgrade();
        let session_end = self.session_end_watch(generation);
        let mut rx = bus.subscribe_filter(SdkEventKind::Connection);
        spawn_im_background(async move {
            'monitor: loop {
                let event = tokio::select! {
                    event = rx.recv() => match event {
                        Ok(ev) => ev,
                        Err(_) => break,
                    },
                    () = session_end.ended() => break,
                };

                let reason = match event {
                    SdkEvent::Connection(ConnectionEvent::Disconnected { reason }) => reason,
                    _ => continue,
                };
                if should_skip_reconnect_for_disconnect_reason(&reason) {
                    continue;
                }
                // 过期的断开事件：它在队列里排着的时候，之前的一次重连已经成功（或网络切换的主动重连
                // 已经连上）。传输层已连上、引擎处于 Connected/Ready 时不重连、也不先报 Reconnecting，
                // 只把引擎状态重发一次（状态快照监视器刚按这条断开把快照置成了 Disconnected）。此前照常
                // 报一次 Reconnecting，退避之后才发现已经连上，界面上平白闪一次「重连中」。
                {
                    let Some(stale_check_client) = client.upgrade() else {
                        break;
                    };
                    if let Some(restored_state) = stale_check_client
                        .restore_state_after_stale_reconnect(generation)
                        .await
                    {
                        tracing::debug!(
                            session_generation = generation,
                            reason = %reason,
                            ?restored_state,
                            "ignored a stale disconnect because transport is already connected"
                        );
                        continue;
                    }
                }
                // 服务端停机（CLOSE(SERVER_SHUTDOWN)、WebSocket 1001、QUIC 应用关闭带来的原因）：
                // 第一次重连前短暂随机等待，而不是按错误的退避节奏。
                let mut after_server_shutdown =
                    flare_core::common::is_server_shutdown_reason(&reason);
                if after_server_shutdown {
                    tracing::info!(
                        session_generation = generation,
                        reason = %reason,
                        "server is shutting down; reconnecting after a jittered pause"
                    );
                }

                let Some(snapshot_client) = client.upgrade() else {
                    break;
                };
                // 这里只取重连参数，不换令牌：断开原因可能随后才到（例如租户不可用的踢线紧跟在
                // 断开之后），换令牌放到退避之后、确认会话仍在时的那次快照里做。
                let (user_id, mut token, interval_secs, max_attempts) =
                    match snapshot_client.reconnect_settings(generation).await {
                        Some(settings) => settings,
                        None => break,
                    };
                drop(snapshot_client);

                let mut attempt = 0u32;
                // 上一次重连被网关按鉴权拒绝时置位：下一次取快照强制走网关换新 token，
                // 不再受本机时钟（token_needs_refresh）左右——否则时钟偏慢会拿着"服务端已过期、
                // 本机以为没过期"的 token 死循环重连。
                let mut force_token_refresh = false;
                loop {
                    let Some(attempt_client) = client.upgrade() else {
                        break;
                    };
                    if !attempt_client.is_generation_current(generation).await {
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
                        attempt_client
                            .mark_current_engine_disconnected(generation)
                            .await;
                        attempt_client
                            .publish_session_event_if_generation(
                                generation,
                                SdkEvent::Connection(ConnectionEvent::Disconnected {
                                    reason: "reconnect attempts exhausted".to_string(),
                                }),
                            )
                            .await;
                        break;
                    }

                    attempt += 1;
                    attempt_client.store_state_snapshot(SdkState::Reconnecting);
                    attempt_client
                        .publish_session_event_if_generation(
                            generation,
                            SdkEvent::Connection(ConnectionEvent::Reconnecting { attempt }),
                        )
                        .await;
                    let wait = reconnect_wait(
                        interval_secs,
                        attempt,
                        std::mem::take(&mut after_server_shutdown),
                    );
                    drop(attempt_client);
                    tokio::select! {
                        () = delay(wait) => {}
                        () = session_end.ended() => break 'monitor,
                    }

                    let Some(reconnect_client) = client.upgrade() else {
                        break;
                    };
                    if let Some(restored_state) = reconnect_client
                        .restore_state_after_stale_reconnect(generation)
                        .await
                    {
                        tracing::debug!(
                            session_generation = generation,
                            attempt,
                            reason = %reason,
                            ?restored_state,
                            "reconciled stale reconnect event because transport is already connected"
                        );
                        break;
                    }

                    // 每次尝试前重取 token：默认无限重试下，一次性快照的 token 过期后
                    // 会永远失败。取不到（如世代切换）时沿用上一枚，保持原有重试节奏，
                    // 由世代校验与 TokenExpired 终态路径负责收尾。
                    if let Some((_, fresh_token, _, _)) = reconnect_client
                        .reconnect_snapshot(generation, force_token_refresh)
                        .await
                    {
                        token = fresh_token;
                    }
                    force_token_refresh = false;

                    let reconnect_result = reconnect_client
                        .reconnect_current_engine(generation, &user_id, &token)
                        .await;
                    // 会话在这次重连期间结束：reconnect_current_engine 已把引擎收尾，结果属于
                    // 已结束的会话，既不算成功也不再重试。
                    if session_end.is_ended() {
                        break 'monitor;
                    }
                    match reconnect_result {
                        Ok(()) => {
                            tracing::info!(
                                session_generation = generation,
                                attempt,
                                "SDK reconnect succeeded"
                            );
                            break;
                        }
                        Err(err) => {
                            // 租户不可用：换令牌、再重连都只会被网关再拒一次。停手，交给应用。
                            if is_tenant_unavailable(&err) {
                                tracing::warn!(
                                    session_generation = generation,
                                    attempt,
                                    error = %err,
                                    "SDK reconnect rejected: tenant unavailable; not retrying"
                                );
                                reconnect_client
                                    .mark_current_engine_disconnected(generation)
                                    .await;
                                reconnect_client
                                    .publish_session_event_if_generation(
                                        generation,
                                        SdkEvent::Connection(ConnectionEvent::KickedOff {
                                            reason: TENANT_UNAVAILABLE.to_string(),
                                        }),
                                    )
                                    .await;
                                break;
                            }
                            // 鉴权类失败（token 过期/无效）时，下一次强制换新 token 再重连。
                            let auth_rejected = is_auth_reconnect_failure(&err);
                            if auth_rejected {
                                force_token_refresh = true;
                            }
                            tracing::warn!(
                                session_generation = generation,
                                attempt,
                                error = %err,
                                force_token_refresh,
                                "SDK reconnect failed"
                            );
                            // 宿主托管令牌：网关按鉴权拒绝时立刻向宿主换一枚。换到了就从头计退避，
                            // 别让一枚早已过期的令牌把恢复拖到退避上限。只在网关明确拒绝之后才换——
                            // 那说明网络是通的；断网时就去换，刷新请求多半也失败，而一次性的刷新
                            // 令牌失败后宿主不会再拿它重试，会话就此作废。
                            if auth_rejected
                                && reconnect_client
                                    .refresh_connect_token_via_host(generation)
                                    .await
                                    .is_some()
                            {
                                attempt = 0;
                                force_token_refresh = false;
                            }
                        }
                    }
                }
            }
            tracing::debug!(
                session_generation = generation,
                session_ended = session_end.is_ended(),
                "SDK reconnect monitor stopped"
            );
        });
    }

    pub(super) async fn sync_foreground_convergence_silent(&self) -> Result<()> {
        let sync = self
            .with_engine_async(|engine| engine.conversation_summary_sync())
            .await?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        let user_id = self.current_user_id().await.unwrap_or_default();
        if user_id.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        sync.sync_foreground_convergence(
            &user_id,
            SyncRunContext::silent_multidevice_private_data(),
        )
        .await
    }

    /// 重连参数（用户、当前令牌、退避间隔、最大次数），不换令牌。
    pub(super) async fn reconnect_settings(
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
            .or_else(|| crate::client::lifecycle::resolve_connect_token(&user_id, None).ok())?;
        let (interval_secs, max_attempts) =
            reconnect_backoff_settings(g.sdk_config.as_ref(), g.configured_config.as_ref());
        Some((user_id, token, interval_secs, max_attempts))
    }

    pub(super) async fn reconnect_snapshot(
        &self,
        generation: u64,
        force_refresh: bool,
    ) -> Option<(String, String, u64, Option<u32>)> {
        let (user_id, mut token, interval_secs, max_attempts) =
            self.reconnect_settings(generation).await?;
        let provider = {
            let g = self.inner.read().await;
            if g.session_generation != generation {
                return None;
            }
            Self::gateway_token_provider_from_inner(&g)
        };
        // 重连前 token 已到期/临期，或上一次重连被网关按鉴权拒绝（force_refresh）：先换新，
        // 否则带着旧 token 重连必被网关拒。force_refresh 绕过本机时钟判定，防时钟偏移死循环。
        // 走刷新令牌换新（接入令牌早过期也能换），是 7x24 的关键。
        if let Some(provider) = provider
            && (force_refresh || token_needs_refresh(&token, 30))
        {
            match self.refresh_tokens_via_gateway(generation, &provider).await {
                Ok(issued) => {
                    token = issued.token;
                }
                Err(err) => {
                    tracing::warn!(%err, "token refresh before reconnect failed; reconnecting with the current token");
                }
            }
        }
        Some((user_id, token, interval_secs, max_attempts))
    }

    /// 宿主托管令牌：向宿主装的续期回调要一枚新的连接令牌。
    ///
    /// 只有拿到的令牌与手上那枚不同才算换到（写回并返回它）；没装回调、会话世代已变、
    /// 宿主返回空或原样返回旧令牌时返回 `None`，调用方照常退避。
    pub(super) async fn refresh_connect_token_via_host(&self, generation: u64) -> Option<String> {
        let (refresher, rejected) = {
            let g = self.inner.read().await;
            if g.session_generation != generation {
                return None;
            }
            // SDK 托管形态由网关签发器负责，不走宿主。
            if Self::gateway_token_provider_from_inner(&g).is_some() {
                return None;
            }
            (g.connect_token_refresher.clone()?, g.connect_token.clone())
        };
        let fresh = refresher.refresh_connect_token().await?;
        let fresh = fresh.trim();
        // 和刷新**之前**手上那枚比，而不是和写回时的比：宿主刷新成功后往往还会另起任务把新令牌
        // 推进来（update_access_token），它可能抢先写入，那时再比就会误判成「没换到」。
        if fresh.is_empty() || rejected.as_deref() == Some(fresh) {
            return None;
        }
        let mut g = self.inner.write().await;
        if g.session_generation != generation {
            return None;
        }
        g.connect_token = Some(fresh.to_string());
        tracing::info!(
            session_generation = generation,
            "reconnect rejected by the gateway; the host supplied a fresh connect token"
        );
        Some(fresh.to_string())
    }

    /// SDK 托管形态下的网关签发器；应用托管（显式 token）时为 `None`。
    pub(super) async fn gateway_token_provider(&self) -> Option<GatewayTokenProvider> {
        let g = self.inner.read().await;
        Self::gateway_token_provider_from_inner(&g)
    }

    pub(super) fn gateway_token_provider_from_inner(
        g: &super::IMClientInner,
    ) -> Option<GatewayTokenProvider> {
        let overlay = g.sdk_config.as_ref()?;
        let auth = overlay.auth.as_ref()?;
        if !auth.sdk_managed() {
            return None;
        }
        let endpoint = auth.token_endpoint.clone()?;
        Some(GatewayTokenProvider::new(
            endpoint,
            Some(Self::resolve_tenant_id(g)),
            overlay.device_id.clone(),
        ))
    }

    /// 用网关换新 token 并重连当前引擎；成功返回 true。
    /// 走网关换新一对令牌：优先拿**刷新令牌**作 Bearer（接入令牌早过期也能换），
    /// 旧网关不下发刷新令牌时回退用接入令牌（服务端两者都认）。成功后回写 access + 轮换后的 refresh。
    pub(super) async fn refresh_tokens_via_gateway(
        &self,
        generation: u64,
        provider: &GatewayTokenProvider,
    ) -> Result<crate::client::token_provider::IssuedAccessToken> {
        let (bearer, tenant_id) = {
            let g = self.inner.read().await;
            if g.session_generation != generation {
                return Err(FlareError::localized(
                    ErrorCode::NotConnected,
                    "session generation changed",
                ));
            }
            // 刷新令牌优先；没有再退回接入令牌（兼容旧网关/旧会话）。
            let bearer = g
                .refresh_token
                .clone()
                .or_else(|| g.connect_token.clone())
                .ok_or_else(|| {
                    FlareError::localized(ErrorCode::NotConnected, "no token to refresh")
                })?;
            (bearer, Self::resolve_tenant_id(&g))
        };
        let issued = provider.refresh(&bearer).await?;
        {
            let mut g = self.inner.write().await;
            if g.session_generation == generation {
                g.connect_token = Some(issued.token.clone());
                // 轮换后的新刷新令牌；网关没下发则保留原来的（别把已有的清成 None）。
                if issued.refresh_token.is_some() {
                    g.refresh_token = issued.refresh_token.clone();
                }
            }
        }
        // 同步 HTTP 侧 bearer（媒体等）。
        let _ = self.sync_gateway_http_context(Some(&tenant_id)).await;
        Ok(issued)
    }

    pub(super) async fn refresh_token_and_reconnect(
        &self,
        generation: u64,
        provider: &GatewayTokenProvider,
    ) -> bool {
        let user_id = {
            let g = self.inner.read().await;
            if g.session_generation != generation {
                return false;
            }
            match g.current_user_id.clone() {
                Some(uid) => uid,
                None => return false,
            }
        };
        let issued = match self.refresh_tokens_via_gateway(generation, provider).await {
            Ok(issued) => issued,
            Err(err) => {
                tracing::warn!(%err, "token refresh via gateway failed");
                return false;
            }
        };
        self.reconnect_current_engine(generation, &user_id, &issued.token)
            .await
            .is_ok()
    }

    /// SDK 托管 token：到期前 `refresh_lead_secs` 秒换新并 `update_access_token`。
    /// 应用托管形态（没配 token_endpoint）什么都不做——刷新是应用的事，核心只抛 TOKEN_EXPIRED。
    ///
    /// 会话结束（登出、断开、重新登录、终止会话、客户端销毁）时立即收手：此前它睡到下一次刷新
    /// （可能是几十分钟后）才发现世代已变，一直挂着。
    pub(super) fn spawn_token_refresh_watcher(&self, generation: u64) {
        let client = self.downgrade();
        let session_end = self.session_end_watch(generation);
        spawn_im_background(async move {
            'watcher: loop {
                let Some(c) = client.upgrade() else { break };
                if !c.is_generation_current(generation).await {
                    break;
                }
                let (provider, token, lead_secs, _tenant_id) = {
                    let g = c.inner.read().await;
                    let provider = Self::gateway_token_provider_from_inner(&g);
                    let lead = g
                        .sdk_config
                        .as_ref()
                        .and_then(|o| o.auth.as_ref())
                        .map(|a| a.refresh_lead().as_secs())
                        .unwrap_or(crate::client::config::SdkAuthConfig::DEFAULT_REFRESH_LEAD_SECS);
                    (
                        provider,
                        g.connect_token.clone(),
                        lead,
                        Self::resolve_tenant_id(&g),
                    )
                };
                let (Some(provider), Some(token)) = (provider, token) else {
                    break;
                };
                let wait_secs = match (jwt_exp_secs(&token), now_unix_secs()) {
                    (Some(exp), Ok(now)) => exp.saturating_sub(lead_secs).saturating_sub(now),
                    // 读不出 exp 的 token 没法安排，10 分钟后再看一眼（token 可能被应用换掉了）。
                    _ => 600u64,
                };
                drop(c);
                tokio::select! {
                    () = delay(Duration::from_secs(wait_secs.max(5))) => {}
                    () = session_end.ended() => break 'watcher,
                }
                let Some(c) = client.upgrade() else { break };
                if !c.is_generation_current(generation).await {
                    break;
                }
                match c.refresh_tokens_via_gateway(generation, &provider).await {
                    Ok(issued) => {
                        tracing::info!(
                            expires_at = issued.expires_at,
                            rotated_refresh = issued.refresh_token.is_some(),
                            "access token refreshed via gateway (refresh-token grant)"
                        );
                    }
                    Err(err) => {
                        // 网关暂时不可达：一分钟后再试；真正过期时 TOKEN_EXPIRED 路径还会兜底。
                        tracing::warn!(%err, "scheduled token refresh failed; retrying in 60s");
                        drop(c);
                        tokio::select! {
                            () = delay(Duration::from_secs(60)) => {}
                            () = session_end.ended() => break 'watcher,
                        }
                    }
                }
            }
            tracing::debug!(
                session_generation = generation,
                "SDK token refresh watcher stopped"
            );
        });
    }

    pub(super) async fn is_generation_current(&self, generation: u64) -> bool {
        self.load_session_generation_snapshot() == generation
    }

    /// The transport may already be connected when the reconnect watcher looks
    /// at a `Disconnected`: the event was stale (left in the queue by an attempt
    /// that later succeeded), or another path reconnected meanwhile (a
    /// network-change reconnect). The transport no longer reconnects on its own
    /// before a send. The watcher checks this before announcing `Reconnecting`
    /// and again after each backoff. Either way the state snapshot watcher has
    /// already stored `Disconnected` for that event (and the watcher may have
    /// published `Reconnecting`), while the engine is still authoritatively
    /// `Connected`/`Ready`; simply ignoring the event strands every UI on the
    /// stale snapshot. Republish the engine state so current and future
    /// subscribers converge on the usable session.
    async fn restore_state_after_stale_reconnect(&self, generation: u64) -> Option<SdkState> {
        let g = self.inner.read().await;
        if g.session_generation != generation {
            return None;
        }
        let engine = g.engine.as_ref()?;
        let state = recovered_connection_state(engine.state(), engine.transport_connected().await)?;
        self.store_state_snapshot(state);
        engine
            .bus()
            .publish(SdkEvent::Connection(ConnectionEvent::StateChanged {
                state,
            }));
        Some(state)
    }

    #[tracing::instrument(skip(self, token), fields(session_generation = generation, user_id = %user_id))]
    pub(super) async fn reconnect_current_engine(
        &self,
        generation: u64,
        user_id: &str,
        token: &str,
    ) -> Result<()> {
        let mut engine = {
            let mut g = self.inner.write().await;
            if g.session_generation != generation {
                return Err(session_ended_during_reconnect());
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
            if self.inner_session_active(&g) {
                match Self::connected_apis_from_inner(&g) {
                    Ok(apis) => self.store_connected_apis_snapshot(apis),
                    Err(err) => {
                        self.clear_session_snapshot();
                        tracing::warn!(%err, "failed to refresh connected API snapshot after reconnect");
                    }
                }
            } else {
                self.clear_session_snapshot();
            }
        } else {
            // 会话在这次重连期间结束：收尾这台引擎，结果不再算数。以前照样返回 Ok，调用方会在已结束
            // 的会话上记一次重连成功（网络切换的主动重连还会向应用报告成功）。
            drop(g);
            engine.deactivate_local_session().await;
            let _ = engine.disconnect().await;
            return Err(session_ended_during_reconnect());
        }
        drop(g);
        if result.is_ok() {
            self.attach_push_session().await;
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
            self.clear_session_snapshot();
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
            g.engine.take()
        };
        self.clear_session_snapshot();
        self.store_state_snapshot(SdkState::Disconnected);
        if let Some(mut e) = engine
            && let Err(err) = e.disconnect().await
        {
            tracing::warn!(%err, "disconnect after terminal event failed");
        }
        self.reconcile_push_in_background();
        true
    }
}

/// 重连监视器的退避间隔（秒，至少 1）与最大次数（`None` 为不限）。
///
/// 优先级与其他连接参数一致：`init()` 的覆盖项 > 构建期 `IMClientBuilder::config` 的配置 >
/// 默认 5 s、不限次数。此前只看覆盖项：经 builder 设置的间隔与次数只到了 flare-core 客户端，
/// SDK 自己的重连循环仍按 5 s、无限次退避。
pub(super) fn reconnect_backoff_settings(
    overlay: Option<&crate::client::lifecycle::SdkConfigOverlay>,
    configured: Option<&crate::client::config::SdkConfig>,
) -> (u64, Option<u32>) {
    let interval_secs = overlay
        .and_then(|c| c.reconnect_interval_secs)
        .or_else(|| configured.and_then(|c| c.reconnect_interval_secs))
        .unwrap_or(5)
        .max(1);
    let max_attempts = overlay
        .and_then(|c| c.max_reconnect_attempts)
        .or_else(|| configured.and_then(|c| c.max_reconnect_attempts))
        .map(|attempts| attempts.max(1));
    (interval_secs, max_attempts)
}

/// Only a stable engine state may clear a reconnect notice. Transport health by
/// itself is not enough while another connect/reconnect transition is active.
pub(super) fn recovered_connection_state(
    state: SdkState,
    transport_connected: bool,
) -> Option<SdkState> {
    if transport_connected && matches!(state, SdkState::Connected | SdkState::Ready) {
        Some(state)
    } else {
        None
    }
}

/// token 在 `lead_secs` 内到期（或已过期、或读不出 exp）就该刷新。
/// 重连失败是否属于鉴权类（token 过期/无效/未授权）——据此决定是否强制换新 token。
pub(super) fn is_auth_reconnect_failure(err: &crate::shared::error::FlareError) -> bool {
    use crate::shared::error::ErrorCode;
    if matches!(
        err.code(),
        Some(ErrorCode::AuthenticationFailed) | Some(ErrorCode::TokenExpired)
    ) {
        return true;
    }
    let lower = err.to_string().to_lowercase();
    lower.contains("authentication_failed")
        || lower.contains("token_expired")
        || lower.contains("token expired")
        || lower.contains("unauthorized")
        || lower.contains("401")
}

pub(super) fn token_needs_refresh(token: &str, lead_secs: u64) -> bool {
    match (jwt_exp_secs(token), now_unix_secs()) {
        (Some(exp), Ok(now)) => exp <= now + lead_secs,
        _ => false,
    }
}

#[cfg(test)]
mod token_refresh_tests {
    use super::*;
    use crate::shared::error::{ErrorCode, FlareError};
    use crate::shared::util::{CoreTokenConfig, generate_core_token};

    #[test]
    fn auth_failures_are_classified_for_forced_refresh() {
        assert!(is_auth_reconnect_failure(&FlareError::localized(
            ErrorCode::AuthenticationFailed,
            "gateway rejected",
        )));
        assert!(is_auth_reconnect_failure(&FlareError::localized(
            ErrorCode::TokenExpired,
            "expired",
        )));
        // 纯网络类失败不该触发强制换 token。
        assert!(!is_auth_reconnect_failure(&FlareError::localized(
            ErrorCode::NotConnected,
            "socket closed",
        )));
        // 文案兜底：错误码不精确时看消息里的鉴权标记。
        assert!(is_auth_reconnect_failure(&FlareError::localized(
            ErrorCode::ConnectionFailed,
            "AUTHENTICATION_FAILED: token 无效或已过期",
        )));
    }

    /// 租户不可用不是鉴权失败：不强制换令牌；重连循环据此停手并上报。
    /// 连接在 CONNECT_ACK 之前被关、服务端没给原因，也不是鉴权失败。
    #[test]
    fn tenant_unavailable_and_reasonless_close_are_not_auth_failures() {
        let tenant = FlareError::localized(
            ErrorCode::PermissionDenied,
            "connect failed primary=wss://x: 错误 [PERMISSION_DENIED] TENANT_UNAVAILABLE: server rejected CONNECT",
        );
        assert!(!is_auth_reconnect_failure(&tenant));
        assert!(is_tenant_unavailable(&tenant));

        let closed = FlareError::connection_failed(
            "server closed the connection before CONNECT_ACK without a reason (heartbeat send failed)",
        );
        assert!(!is_auth_reconnect_failure(&closed));
        assert!(!is_tenant_unavailable(&closed));
    }

    /// 租户不可用按错误码识别，与错误文本无关：文本里带着 TENANT_UNAVAILABLE 的连接失败不算，
    /// 文本没有它的 PERMISSION_DENIED 算。
    #[test]
    fn tenant_unavailable_is_recognised_by_the_error_code_not_the_text() {
        assert!(!is_tenant_unavailable(&FlareError::connection_failed(
            "connect failed primary=wss://tenant_unavailable.example: TENANT_UNAVAILABLE",
        )));
        assert!(is_tenant_unavailable(&FlareError::localized(
            ErrorCode::PermissionDenied,
            "connect failed primary=wss://x: server rejected CONNECT",
        )));
    }

    /// 服务端停机后的第一次重连只等 1–5 s 的随机一段，不按配置的退避（这里 30 s）；
    /// 之后的尝试、以及非停机的断开照常按退避。
    #[test]
    fn the_first_reconnect_after_a_server_shutdown_waits_a_short_jittered_pause() {
        use flare_core::common::shutdown::{
            SERVER_SHUTDOWN_RECONNECT_MAX, SERVER_SHUTDOWN_RECONNECT_MIN,
        };
        let mut distinct = std::collections::HashSet::new();
        for _ in 0..100 {
            let wait = reconnect_wait(30, 1, true);
            assert!(
                (SERVER_SHUTDOWN_RECONNECT_MIN..=SERVER_SHUTDOWN_RECONNECT_MAX).contains(&wait),
                "{wait:?}"
            );
            distinct.insert(wait);
        }
        assert!(distinct.len() > 20, "the pause must be jittered");
        let normal = reconnect_wait(30, 1, false);
        assert!(
            (Duration::from_secs(24)..=Duration::from_secs(30)).contains(&normal),
            "{normal:?}"
        );
    }

    /// 服务端暂时放不了行（如网关读不到租户状态）：既不换令牌，也不停手，照常退避重连。
    #[test]
    fn transient_rejection_is_neither_an_auth_failure_nor_tenant_unavailable() {
        let transient = FlareError::localized(
            ErrorCode::ServiceUnavailable,
            "connect failed primary=wss://x: 错误 [SERVICE_UNAVAILABLE] SERVICE_UNAVAILABLE: server rejected CONNECT",
        );
        assert!(!is_auth_reconnect_failure(&transient));
        assert!(!is_tenant_unavailable(&transient));
    }

    fn token(ttl_secs: u64) -> String {
        generate_core_token(&CoreTokenConfig {
            secret: "a-strong-shared-secret-with-more-than-32-bytes!".into(),
            issuer: "flare-im-core".into(),
            user_id: "u".into(),
            ttl_secs,
            device_id: None,
            tenant_id: None,
        })
        .unwrap()
    }

    #[test]
    fn needs_refresh_only_inside_the_lead_window() {
        // generate_core_token 把 ttl 抬到至少 60s：60s 内到期 + 30s 提前量 = 不该刷；提前量 120s 就该刷。
        assert!(!token_needs_refresh(&token(60), 30));
        assert!(token_needs_refresh(&token(60), 120));
        assert!(
            !token_needs_refresh("garbage", 120),
            "读不出 exp 不能误判成要刷"
        );
    }
}
