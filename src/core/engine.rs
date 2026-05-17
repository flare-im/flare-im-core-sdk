use std::sync::Arc;
use std::sync::Arc as StdArc;

use tokio::sync::RwLock;

use crate::application::event_deduper::EventDeduper;
use crate::application::message_deduper::MessageDeduper;
use crate::core::{SessionSyncRunner, SyncManager, SyncResponseHandler, SyncRunContext};
use crate::error::FlareError;
use crate::event::{ConnectionEvent as SdkConnectionEvent, EventBus, SdkEvent};
use crate::fsm::{ConnectionEvent, ConnectionFsm, ConnectionState};
use crate::middleware::MiddlewareChain;
use crate::protocol::{Codec, PacketSender};
use crate::reliable_queue::ReliableSendQueue;
use crate::store::StoreProvider;
use crate::transport::{SocketHandler, SocketTransport};

/// 对外暴露的连接状态（与 fsm::ConnectionState 对齐，便于 UI 展示）
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdkState {
    Disconnected,
    Connecting,
    Connected,
    Ready,
    Reconnecting,
}

impl From<crate::fsm::ConnectionState> for SdkState {
    fn from(s: crate::fsm::ConnectionState) -> Self {
        use crate::fsm::ConnectionState as S;
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
    codec: Arc<dyn Codec>,
    _chain: Arc<MiddlewareChain>,
    reliable_queue: Option<Arc<ReliableSendQueue>>,
    connection_state: StdArc<RwLock<ConnectionState>>,
    event_deduper: EventDeduper,
    message_deduper: MessageDeduper,
}

impl SdkEngine {
    /// 创建引擎。连接就绪后由 [connect] 内 [bootstrap] 激活同步；同步状态仅通过 [EventBus] 的同步回调获取。
    /// `sync_response_handler` / `session_sync` 通常为同一 application SyncProtocolAdapter 的 Arc。
    pub(crate) fn new(
        stores: StoreProvider,
        _chain: MiddlewareChain,
        transport: SocketTransport,
        current_user_id: crate::core::CurrentUserIdStore,
        codec: Arc<dyn Codec>,
        bus: EventBus,
        sync_response_handler: Option<Arc<dyn SyncResponseHandler>>,
        session_sync: Option<Arc<dyn SessionSyncRunner>>,
        event_deduper: EventDeduper,
        message_deduper: MessageDeduper,
    ) -> Self {
        let sender = transport.sender().clone();
        let reliable_queue = stores.pending_sends().map(|(reader, writer)| {
            Arc::new(ReliableSendQueue::new(
                reader,
                writer,
                sender.clone(),
                stores.messages.clone(),
                current_user_id.clone(),
                bus.clone(),
                None,
                None,
            ))
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
            codec,
            _chain: Arc::new(_chain),
            reliable_queue,
            connection_state: StdArc::new(RwLock::new(ConnectionState::Disconnected)),
            event_deduper,
            message_deduper,
        }
    }

    fn publish_state(&self, state: ConnectionState) {
        self.bus
            .publish(SdkEvent::Connection(SdkConnectionEvent::StateChanged {
                state: state.into(),
            }));
    }

    async fn transition(&self, event: ConnectionEvent) {
        let mut guard = self.connection_state.write().await;
        match ConnectionFsm::transition(*guard, &event) {
            Ok(next) => {
                *guard = next;
                drop(guard);
                self.publish_state(next);
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
    ) -> crate::error::Result<()> {
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
    pub async fn connect(&mut self, user_id: &str, token: &str) -> crate::error::Result<()> {
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

    pub async fn reconnect(&mut self, user_id: &str, token: &str) -> crate::error::Result<()> {
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

    pub async fn disconnect(&mut self) -> crate::error::Result<()> {
        self.transition(ConnectionEvent::DisconnectRequested).await;
        self.sync_manager.stop_sync();
        self.transport.disconnect().await?;
        *self.current_user_id.write().await = String::new();
        {
            let mut guard = self.connection_state.write().await;
            *guard = ConnectionState::Disconnected;
            drop(guard);
        }
        self.publish_state(ConnectionState::Disconnected);
        Ok(())
    }

    pub async fn bootstrap(&mut self, sync_run: SyncRunContext) -> crate::error::Result<()> {
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
        self.connection_state
            .try_read()
            .map(|g| (*g).into())
            .unwrap_or(SdkState::Disconnected)
    }

    pub async fn transport_connected(&self) -> bool {
        self.transport.is_connected().await
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

    pub fn sync_manager(&self) -> Arc<SyncManager> {
        self.sync_manager.clone()
    }

    /// 单会话消息同步与已读上报（会话列表同步由同步引擎内部执行，不暴露）
    pub fn session_sync_runner(&self) -> Option<Arc<dyn SessionSyncRunner>> {
        self.session_sync.clone()
    }

    pub fn reliable_queue(&self) -> Option<Arc<ReliableSendQueue>> {
        self.reliable_queue.clone()
    }

    /// 当前已连接用户 ID（未连接时为空）
    pub async fn current_user_id(&self) -> String {
        self.current_user_id.read().await.clone()
    }
}

use crate::core::Dispatcher;
