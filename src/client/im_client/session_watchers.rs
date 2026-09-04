use std::time::Duration;

use crate::FlareError;
use crate::kernel::event::{ConnectionEvent, EventBus, SdkEvent, SdkEventKind};
use crate::kernel::{SdkState, SyncRunContext};
use crate::shared::error::{ErrorCode, Result};
use crate::client::token_provider::{GatewayTokenProvider, jwt_exp_secs};
use crate::shared::util::{delay, now_unix_secs};

use super::{
    IMClient, reconnect_delay_secs, should_skip_reconnect_for_disconnect_reason,
    spawn_im_background,
};

impl IMClient {
    pub(super) fn spawn_state_snapshot_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.downgrade();
        // 只消费连接事件——按 Kind 过滤使总线不再为本订阅者深克隆消息批等热事件。
        let mut rx = bus.subscribe_filter(SdkEventKind::Connection);
        spawn_im_background(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(ev) => ev,
                    Err(_) => break,
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
        });
    }

    /// 中断连接会话监听器
    pub(super) fn spawn_terminal_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.downgrade();
        let mut rx = bus.subscribe_filter(SdkEventKind::Connection);
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
                let Some(client) = client.upgrade() else {
                    break;
                };
                // SDK 托管 token：过期不是终局，先刷新再重连；刷新失败才按终局处理。
                if reason.starts_with("token_expired:")
                    && let Some(provider) = client.gateway_token_provider().await
                    && client.refresh_token_and_reconnect(generation, &provider).await
                {
                    tracing::info!(session_generation = generation, "token expired: refreshed via gateway and reconnected");
                    continue;
                }
                let applied = client.terminate_session_if_generation(generation).await;
                if applied {
                    tracing::warn!(session_generation = generation, reason = %reason, "session terminated by terminal connection event");
                }
                break;
            }
        });
    }

    pub(super) fn spawn_reconnect_session_watcher(&self, generation: u64, bus: EventBus) {
        let client = self.downgrade();
        let mut rx = bus.subscribe_filter(SdkEventKind::Connection);
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

                let Some(snapshot_client) = client.upgrade() else {
                    break;
                };
                let (user_id, mut token, interval_secs, max_attempts) =
                    match snapshot_client.reconnect_snapshot(generation).await {
                        Some(snapshot) => snapshot,
                        None => break,
                    };
                drop(snapshot_client);

                let mut attempt = 0u32;
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
                    let delay_secs = reconnect_delay_secs(interval_secs, attempt);
                    drop(attempt_client);
                    delay(Duration::from_secs(delay_secs)).await;

                    let Some(reconnect_client) = client.upgrade() else {
                        break;
                    };
                    if reconnect_client
                        .is_current_transport_connected(generation)
                        .await
                    {
                        tracing::debug!(
                            session_generation = generation,
                            attempt,
                            reason = %reason,
                            "skip stale reconnect event because transport is already connected"
                        );
                        break;
                    }

                    // 每次尝试前重取 token：默认无限重试下，一次性快照的 token 过期后
                    // 会永远失败。取不到（如世代切换）时沿用上一枚，保持原有重试节奏，
                    // 由世代校验与 TokenExpired 终态路径负责收尾。
                    if let Some((_, fresh_token, _, _)) =
                        reconnect_client.reconnect_snapshot(generation).await
                    {
                        token = fresh_token;
                    }

                    match reconnect_client
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

    pub(super) async fn reconnect_snapshot(
        &self,
        generation: u64,
    ) -> Option<(String, String, u64, Option<u32>)> {
        let g = self.inner.read().await;
        if g.session_generation != generation {
            return None;
        }
        let user_id = g.current_user_id.clone()?;
        let mut token = g
            .connect_token
            .clone()
            .or_else(|| crate::client::lifecycle::resolve_connect_token(&user_id, None).ok())?;
        let provider = Self::gateway_token_provider_from_inner(&g);
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
        drop(g);
        // 重连前 token 已到期/临期：先换新，否则带着旧 token 重连必被网关拒。
        if let Some(provider) = provider
            && token_needs_refresh(&token, 30)
        {
            match provider.refresh(&token).await {
                Ok(issued) => {
                    let mut g = self.inner.write().await;
                    if g.session_generation == generation {
                        g.connect_token = Some(issued.token.clone());
                        token = issued.token;
                    }
                }
                Err(err) => {
                    tracing::warn!(%err, "token refresh before reconnect failed; reconnecting with the current token");
                }
            }
        }
        Some((user_id, token, interval_secs, max_attempts))
    }

    /// SDK 托管形态下的网关签发器；应用托管（显式 token）时为 `None`。
    pub(super) async fn gateway_token_provider(&self) -> Option<GatewayTokenProvider> {
        let g = self.inner.read().await;
        Self::gateway_token_provider_from_inner(&g)
    }

    pub(super) fn gateway_token_provider_from_inner(g: &super::IMClientInner) -> Option<GatewayTokenProvider> {
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
    pub(super) async fn refresh_token_and_reconnect(&self, generation: u64, provider: &GatewayTokenProvider) -> bool {
        let (user_id, token, tenant_id) = {
            let g = self.inner.read().await;
            if g.session_generation != generation {
                return false;
            }
            let Some(user_id) = g.current_user_id.clone() else { return false };
            let Some(token) = g.connect_token.clone() else { return false };
            (user_id, token, Self::resolve_tenant_id(&g))
        };
        let issued = match provider.refresh(&token).await {
            Ok(issued) => issued,
            Err(err) => {
                tracing::warn!(%err, "token refresh via gateway failed");
                return false;
            }
        };
        if let Err(err) = self.update_access_token(issued.token.clone(), Some(&tenant_id)).await {
            tracing::warn!(%err, "applying refreshed token failed");
            return false;
        }
        self.reconnect_current_engine(generation, &user_id, &issued.token)
            .await
            .is_ok()
    }

    /// SDK 托管 token：到期前 `refresh_lead_secs` 秒换新并 `update_access_token`。
    /// 应用托管形态（没配 token_endpoint）什么都不做——刷新是应用的事，核心只抛 TOKEN_EXPIRED。
    pub(super) fn spawn_token_refresh_watcher(&self, generation: u64) {
        let client = self.downgrade();
        spawn_im_background(async move {
            loop {
                let Some(c) = client.upgrade() else { break };
                if !c.is_generation_current(generation).await {
                    break;
                }
                let (provider, token, lead_secs, tenant_id) = {
                    let g = c.inner.read().await;
                    let provider = Self::gateway_token_provider_from_inner(&g);
                    let lead = g
                        .sdk_config
                        .as_ref()
                        .and_then(|o| o.auth.as_ref())
                        .map(|a| a.refresh_lead().as_secs())
                        .unwrap_or(crate::client::config::SdkAuthConfig::DEFAULT_REFRESH_LEAD_SECS);
                    (provider, g.connect_token.clone(), lead, Self::resolve_tenant_id(&g))
                };
                let (Some(provider), Some(token)) = (provider, token) else { break };
                let wait_secs = match (jwt_exp_secs(&token), now_unix_secs()) {
                    (Some(exp), Ok(now)) => exp.saturating_sub(lead_secs).saturating_sub(now),
                    // 读不出 exp 的 token 没法安排，10 分钟后再看一眼（token 可能被应用换掉了）。
                    _ => 600u64,
                };
                drop(c);
                delay(Duration::from_secs(wait_secs.max(5))).await;
                let Some(c) = client.upgrade() else { break };
                if !c.is_generation_current(generation).await {
                    break;
                }
                match provider.refresh(&token).await {
                    Ok(issued) => {
                        if let Err(err) = c.update_access_token(issued.token, Some(&tenant_id)).await {
                            tracing::warn!(%err, "applying refreshed token failed");
                        } else {
                            tracing::info!(expires_at = issued.expires_at, "access token refreshed via gateway");
                        }
                    }
                    Err(err) => {
                        // 网关暂时不可达：一分钟后再试；真正过期时 TOKEN_EXPIRED 路径还会兜底。
                        tracing::warn!(%err, "scheduled token refresh failed; retrying in 60s");
                        delay(Duration::from_secs(60)).await;
                    }
                }
            }
        });
    }

    pub(super) async fn is_generation_current(&self, generation: u64) -> bool {
        self.load_session_generation_snapshot() == generation
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
            if Self::is_active_session_state(state) {
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
        self.clear_session_snapshot();
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
        true
    }
}

/// token 在 `lead_secs` 内到期（或已过期、或读不出 exp）就该刷新。
pub(super) fn token_needs_refresh(token: &str, lead_secs: u64) -> bool {
    match (jwt_exp_secs(token), now_unix_secs()) {
        (Some(exp), Ok(now)) => exp <= now + lead_secs,
        _ => false,
    }
}

#[cfg(test)]
mod token_refresh_tests {
    use super::*;
    use crate::shared::util::{CoreTokenConfig, generate_core_token};

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
        assert!(!token_needs_refresh("garbage", 120), "读不出 exp 不能误判成要刷");
    }
}
