use std::sync::Arc;
use std::sync::Arc as StdArc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use flare_core::common::{HeartbeatAppState, HeartbeatConfig};
use tokio::sync::RwLock;

use crate::application::notification::NotificationInboundPipeline;
use crate::application::services::EventDeduper;
use crate::application::services::MessageDeduper;
use crate::core::event::{ConnectionEvent as SdkConnectionEvent, EventBus, SdkEvent};
use crate::core::{
    ConnectionEvent, ConnectionFsm, ConnectionState, ConversationSummarySync, ReliableSendQueue,
    ReliableSendQueueConfig, SessionSyncRunner, SyncManager, SyncResponseHandler, SyncRunContext,
};
use crate::extension::middleware::MiddlewareChain;
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::protocol::{Codec, PacketSender};
use crate::infrastructure::transport::{SocketHandler, SocketTransport};
use crate::shared::error::FlareError;

/// 对外暴露的连接状态（与 core FSM ConnectionState 对齐，便于 UI 展示）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdkState {
    Disconnected,
    Connecting,
    Connected,
    Ready,
    Reconnecting,
}

impl SdkState {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            SdkState::Disconnected => 0,
            SdkState::Connecting => 1,
            SdkState::Connected => 2,
            SdkState::Ready => 3,
            SdkState::Reconnecting => 4,
        }
    }

    pub(crate) fn from_u8(value: u8) -> Self {
        match value {
            1 => SdkState::Connecting,
            2 => SdkState::Connected,
            3 => SdkState::Ready,
            4 => SdkState::Reconnecting,
            _ => SdkState::Disconnected,
        }
    }
}

impl From<ConnectionState> for SdkState {
    fn from(s: ConnectionState) -> Self {
        use ConnectionState as S;
        match s {
            S::Disconnected => SdkState::Disconnected,
            S::Connecting => SdkState::Connecting,
            S::Connected => SdkState::Connected,
            S::Ready => SdkState::Ready,
            S::Reconnecting => SdkState::Reconnecting,
        }
    }
}

pub struct SdkEngine {
    stores: StoreProvider,
    bus: EventBus,
    sender: Arc<PacketSender>,
    transport: SocketTransport,
    current_user_id: crate::core::CurrentUserIdStore,
    sync_manager: Arc<SyncManager>,
    sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
    session_sync: Option<Arc<dyn SessionSyncRunner>>,
    conversation_summary_sync: Option<Arc<dyn ConversationSummarySync>>,
    codec: Arc<dyn Codec>,
    chain: Arc<MiddlewareChain>,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    connection_state: StdArc<RwLock<ConnectionState>>,
    state_snapshot: AtomicU8,
    event_deduper: EventDeduper,
    message_deduper: MessageDeduper,
    notification_pipeline: NotificationInboundPipeline,
}

pub(crate) struct SdkEngineConfig {
    pub stores: StoreProvider,
    pub chain: Arc<MiddlewareChain>,
    pub transport: SocketTransport,
    pub current_user_id: crate::core::CurrentUserIdStore,
    pub codec: Arc<dyn Codec>,
    pub bus: EventBus,
    pub sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
    pub session_sync: Option<Arc<dyn SessionSyncRunner>>,
    pub conversation_summary_sync: Option<Arc<dyn ConversationSummarySync>>,
    pub event_deduper: EventDeduper,
    pub message_deduper: MessageDeduper,
    pub notification_pipeline: NotificationInboundPipeline,
    pub ack_timeout_secs: Option<u64>,
    pub ack_max_retries: Option<u32>,
    pub ack_max_in_flight: Option<usize>,
}

impl SdkEngine {
    /// 创建引擎。连接就绪后由 [connect] 内 [bootstrap] 激活同步；同步状态仅通过 [EventBus] 的同步回调获取。
    /// `sync_response_handler` / `session_sync` 通常为同一 application SyncProtocolAdapter 的 Arc。
    pub(crate) fn new(config: SdkEngineConfig) -> Self {
        let SdkEngineConfig {
            stores,
            chain,
            transport,
            current_user_id,
            codec,
            bus,
            sync_response_handler,
            session_sync,
            conversation_summary_sync,
            event_deduper,
            message_deduper,
            notification_pipeline,
            ack_timeout_secs,
            ack_max_retries,
            ack_max_in_flight,
        } = config;
        let sender = transport.sender().clone();
        let reliable_queue = stores.pending_sends().map(|(reader, writer)| {
            Arc::new(ReliableSendQueue::new(ReliableSendQueueConfig {
                pending_reader: reader,
                pending_writer: writer,
                sender: sender.clone(),
                message_store: stores.messages.clone(),
                conversation_store: stores.conversations.clone(),
                current_user_id: current_user_id.clone(),
                bus: bus.clone(),
                timeout_secs: ack_timeout_secs,
                max_retries: ack_max_retries,
                max_in_flight: ack_max_in_flight,
            }))
        });
        Self {
            stores,
            bus,
            sender,
            transport,
            current_user_id,
            sync_manager: Arc::new(SyncManager::new()),
            sync_response_handler,
            session_sync,
            conversation_summary_sync,
            codec,
            chain,
            reliable_queue,
            connection_state: StdArc::new(RwLock::new(ConnectionState::Disconnected)),
            state_snapshot: AtomicU8::new(SdkState::Disconnected.as_u8()),
            event_deduper,
            message_deduper,
            notification_pipeline,
        }
    }

    fn publish_state(&self, state: ConnectionState) {
        self.bus
            .publish(SdkEvent::Connection(SdkConnectionEvent::StateChanged {
                state: state.into(),
            }));
    }

    fn store_state_snapshot(&self, state: ConnectionState) {
        self.state_snapshot
            .store(SdkState::from(state).as_u8(), Ordering::Release);
    }

    async fn transition(&self, event: ConnectionEvent) {
        let mut guard = self.connection_state.write().await;
        match ConnectionFsm::transition(*guard, &event) {
            Ok(next) => {
                *guard = next;
                self.store_state_snapshot(next);
                drop(guard);
                self.publish_state(next);
                // FSM `Connected` 与 EventBus `ConnectionEvent::Connected` 不同名域；
                // Tauri `im://connected` 依赖后者，仅发 StateChanged 会导致前端一直停在「连接中」。
                if next == ConnectionState::Connected {
                    self.bus
                        .publish(SdkEvent::Connection(SdkConnectionEvent::Connected));
                }
            }
            Err(e) => {
                tracing::warn!(%e, "connection FSM transition rejected");
            }
        }
    }

    async fn force_disconnected_after_connect_failure(&self, user_id: &str, error: &FlareError) {
        self.sync_manager.stop_sync();
        if let Err(disconnect_error) = self.transport.disconnect().await {
            tracing::warn!(
                user_id,
                error = %disconnect_error,
                "transport cleanup after connection failure failed"
            );
        }
        *self.current_user_id.write().await = String::new();
        {
            let mut guard = self.connection_state.write().await;
            *guard = ConnectionState::Disconnected;
            self.store_state_snapshot(ConnectionState::Disconnected);
        }
        self.publish_state(ConnectionState::Disconnected);
        tracing::warn!(
            user_id,
            error = %error,
            "connection attempt failed; engine state reset to Disconnected"
        );
    }

    async fn connect_after_state_transition(
        &mut self,
        user_id: &str,
        token: &str,
        sync_run: SyncRunContext,
    ) -> crate::shared::error::Result<()> {
        let ready = Arc::new(tokio::sync::Notify::new());
        let listener = Arc::new(SocketHandler::new(
            Arc::new(Dispatcher::new(
                self.bus.clone(),
                self.reliable_queue.clone(),
                self.sync_response_handler.clone(),
                self.session_sync.clone(),
                Some(self.stores.clone()),
                self.current_user_id.clone(),
                self.event_deduper.clone(),
                self.message_deduper.clone(),
                self.notification_pipeline.clone(),
            )),
            self.codec.clone(),
            ready.clone(),
        ));
        if let Err(error) = self
            .transport
            .connect(user_id, token, listener, ready)
            .await
        {
            self.force_disconnected_after_connect_failure(user_id, &error)
                .await;
            return Err(error);
        }
        *self.current_user_id.write().await = user_id.to_string();
        if let Some(queue) = &self.reliable_queue {
            let _ = queue.recover_pending_for_current_user().await;
        }
        self.transition(ConnectionEvent::Connected).await;
        if let Err(error) = self.bootstrap(sync_run).await {
            self.force_disconnected_after_connect_failure(user_id, &error)
                .await;
            return Err(error);
        }
        self.transition(ConnectionEvent::BootstrapDone).await;
        Ok(())
    }

    /// 连接服务器。同一用户已就绪时幂等返回；正在连接中或已连接为其他用户时返回错误，避免重复建连导致服务端踢线。
    pub async fn connect(
        &mut self,
        user_id: &str,
        token: &str,
    ) -> crate::shared::error::Result<()> {
        let (state, current_uid) = {
            let s = *self.connection_state.read().await;
            let uid = self.current_user_id.read().await.clone();
            (s, uid)
        };
        match state {
            ConnectionState::Ready if current_uid == user_id => {
                tracing::debug!(%user_id, "connect idempotent: already connected as same user");
                return Ok(());
            }
            ConnectionState::Ready => {
                return Err(FlareError::general_error(format!(
                    "already connected as {}, disconnect first before connecting as {}",
                    current_uid, user_id
                )));
            }
            ConnectionState::Connecting
            | ConnectionState::Connected
            | ConnectionState::Reconnecting => {
                return Err(FlareError::general_error(
                    "connect already in progress or reconnecting, wait for it to finish or fail",
                ));
            }
            ConnectionState::Disconnected => {}
        }

        self.transition(ConnectionEvent::ConnectRequested).await;
        self.connect_after_state_transition(user_id, token, SyncRunContext::initial_login())
            .await
    }

    pub async fn mark_transport_disconnected(&self) {
        self.transition(ConnectionEvent::Disconnected).await;
    }

    pub async fn reconnect(
        &mut self,
        user_id: &str,
        token: &str,
    ) -> crate::shared::error::Result<()> {
        let state = *self.connection_state.read().await;
        match state {
            ConnectionState::Ready => self.transition(ConnectionEvent::ReconnectRequested).await,
            ConnectionState::Disconnected => {
                self.transition(ConnectionEvent::ConnectRequested).await
            }
            ConnectionState::Reconnecting => {}
            ConnectionState::Connecting | ConnectionState::Connected => {
                return Err(FlareError::general_error(
                    "connect already in progress, skip reconnect",
                ));
            }
        }

        self.sync_manager.stop_sync();
        self.transport.disconnect().await?;
        self.connect_after_state_transition(user_id, token, SyncRunContext::reconnect())
            .await
    }

    pub(crate) async fn deactivate_local_session(&self) {
        self.sync_manager.stop_sync();
        if let Some(queue) = &self.reliable_queue {
            queue.shutdown();
        }
        *self.current_user_id.write().await = String::new();
    }

    pub async fn disconnect(&mut self) -> crate::shared::error::Result<()> {
        self.transition(ConnectionEvent::DisconnectRequested).await;
        self.deactivate_local_session().await;
        self.transport.disconnect().await?;
        {
            let mut guard = self.connection_state.write().await;
            *guard = ConnectionState::Disconnected;
            self.store_state_snapshot(ConnectionState::Disconnected);
            drop(guard);
        }
        self.publish_state(ConnectionState::Disconnected);
        Ok(())
    }

    pub async fn bootstrap(
        &mut self,
        sync_run: SyncRunContext,
    ) -> crate::shared::error::Result<()> {
        let user_id = self.current_user_id.read().await.clone();
        if user_id.is_empty() {
            return Ok(());
        }
        self.sync_manager.run_with_context(
            &user_id,
            sync_run,
            self.stores.clone(),
            self.bus.clone(),
        );
        Ok(())
    }

    /// 当前连接状态（由 FSM 驱动）
    pub fn state(&self) -> SdkState {
        SdkState::from_u8(self.state_snapshot.load(Ordering::Acquire))
    }

    pub async fn transport_connected(&self) -> bool {
        self.transport.is_connected().await
    }

    pub async fn update_heartbeat_config(&self, config: HeartbeatConfig) -> crate::Result<()> {
        self.transport.update_heartbeat_config(config).await
    }

    pub async fn set_heartbeat_app_state(&self, state: HeartbeatAppState) -> crate::Result<()> {
        self.transport.set_heartbeat_app_state(state).await
    }

    pub async fn set_heartbeat_nat_timeout(&self, timeout: Option<Duration>) -> crate::Result<()> {
        self.transport.set_heartbeat_nat_timeout(timeout).await
    }

    pub async fn heartbeat_effective_interval(&self) -> Option<Duration> {
        self.transport.heartbeat_effective_interval().await
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn sender(&self) -> &Arc<PacketSender> {
        &self.sender
    }

    pub fn stores(&self) -> &StoreProvider {
        &self.stores
    }

    pub fn middleware_chain(&self) -> Arc<MiddlewareChain> {
        self.chain.clone()
    }

    pub fn sync_manager(&self) -> Arc<SyncManager> {
        self.sync_manager.clone()
    }

    /// 单会话消息同步与已读上报（会话列表同步由同步引擎内部执行，不暴露）
    pub fn session_sync_runner(&self) -> Option<Arc<dyn SessionSyncRunner>> {
        self.session_sync.clone()
    }

    pub fn conversation_summary_sync(&self) -> Option<Arc<dyn ConversationSummarySync>> {
        self.conversation_summary_sync.clone()
    }

    pub(crate) fn reliable_queue(&self) -> Option<Arc<ReliableSendQueue>> {
        self.reliable_queue.clone()
    }

    /// 当前已连接用户 ID（未连接时为空）
    pub async fn current_user_id(&self) -> String {
        self.current_user_id.read().await.clone()
    }
}

use crate::core::Dispatcher;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::RwLock;
    use tokio::time::{Duration, timeout};

    use super::{SdkEngine, SdkEngineConfig};
    use crate::application::notification::{
        NotificationHandlerRegistry, NotificationInboundPipeline,
    };
    use crate::application::services::{EventDeduper, MessageDeduper};
    use crate::client::config::SdkConfig;
    use crate::core::event::EventBus;
    use crate::extension::middleware::MiddlewareChain;
    use crate::infrastructure::persistence::memory_im::in_memory_im_provider;
    use crate::infrastructure::protocol::{Codec, ProtobufCodec};
    use crate::infrastructure::transport::SocketTransport;
    use crate::model::IMMessage;

    fn test_engine(current_user_id: crate::core::CurrentUserIdStore) -> SdkEngine {
        let stores = in_memory_im_provider();
        let bus = EventBus::new();
        let message_deduper = MessageDeduper::new(Some(8));
        SdkEngine::new(SdkEngineConfig {
            stores,
            chain: Arc::new(MiddlewareChain::new()),
            transport: SocketTransport::new(SdkConfig::default()),
            current_user_id,
            codec: Arc::new(ProtobufCodec) as Arc<dyn Codec>,
            bus: bus.clone(),
            sync_response_handler: None,
            session_sync: None,
            conversation_summary_sync: None,
            event_deduper: EventDeduper::new(Some(8)),
            message_deduper: message_deduper.clone(),
            notification_pipeline: NotificationInboundPipeline::new(
                Arc::new(NotificationHandlerRegistry::new()),
                message_deduper,
                bus,
            ),
            ack_timeout_secs: Some(60),
            ack_max_retries: Some(3),
            ack_max_in_flight: Some(4),
        })
    }

    #[tokio::test]
    async fn deactivate_local_session_clears_user_and_stops_reliable_queue() {
        let current_user_id = Arc::new(RwLock::new("u1".to_string()));
        let engine = test_engine(current_user_id.clone());
        let queue = engine
            .reliable_queue()
            .expect("in-memory provider exposes pending send store");

        engine.deactivate_local_session().await;

        assert_eq!(engine.current_user_id().await, "");
        assert_eq!(current_user_id.read().await.as_str(), "");

        let mut message = IMMessage::new(flare_proto::common::Message::default());
        message.client_msg_id = "client-after-logout".to_string();
        message.conversation_id = "conv-after-logout".to_string();
        message.sender_id = "u1".to_string();

        let result = timeout(Duration::from_millis(200), queue.enqueue(message)).await;
        assert!(
            matches!(result, Ok(Err(_))),
            "shutdown queue must reject stale enqueue attempts"
        );
    }
}
