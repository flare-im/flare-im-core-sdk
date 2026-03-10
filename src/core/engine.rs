use std::sync::Arc;

use tokio::sync::{Notify, RwLock};
use tracing::info;

use crate::core::lifecycle::{SdkState, StateManager};
use crate::core::dispatcher::Dispatcher;
use crate::core::router::Router;
use crate::event::{EventBus, SdkEvent};
use crate::error::{SdkError, Result};
use crate::middleware::MiddlewareChain;
use crate::protocol::{PacketSender, ProtobufCodec};
use crate::store::StoreProvider;
use crate::sync::SyncManager;
use crate::transport::{SocketTransport, SocketHandler};

/// 当前用户 ID 提供者（连接时写入，断开时清空，供 MessageApi/ConversationApi 使用）
pub type CurrentUserIdStore = Arc<RwLock<String>>;

/// SDK 核心引擎 — 组装并驱动所有子系统
///
/// ```text
///                          ┌──────────────────┐
///    IMClient ──────────── │    SdkEngine      │
///                          │                   │
///      ┌───────────────────┤  state_manager    │
///      │                   │  event_bus        │
///      │   ┌───────────────┤  dispatcher       │
///      │   │               │  router           │
///      │   │  ┌────────────┤  sync_manager     │
///      │   │  │            │  middleware_chain  │
///      │   │  │            │  ws_transport      │
///      │   │  │            │  packet_sender     │
///      │   │  │            └──────────────────┘
/// ```
pub struct SdkEngine {
    pub(crate) state: Arc<StateManager>,
    pub(crate) bus: EventBus,
    pub(crate) dispatcher: Arc<Dispatcher>,
    pub(crate) router: Router,
    pub(crate) sync_manager: SyncManager,
    pub(crate) chain: Arc<MiddlewareChain>,
    pub(crate) transport: SocketTransport,
    pub(crate) sender: Arc<PacketSender>,
    pub(crate) stores: Arc<StoreProvider>,
    pub(crate) current_user_id: CurrentUserIdStore,
    router_handle: Option<tokio::task::JoinHandle<()>>,
}

impl SdkEngine {
    pub fn new(
        stores: StoreProvider,
        chain: MiddlewareChain,
        transport: SocketTransport,
        current_user_id: CurrentUserIdStore,
    ) -> Self {
        let state = Arc::new(StateManager::new());
        let bus = EventBus::new();
        let chain = Arc::new(chain);

        let dispatcher = Arc::new(Dispatcher::new(bus.clone(), chain.clone()));

        let sender = transport.sender().clone();

        let router = Router::new(
            stores.messages.clone(),
            stores.conversations.clone(),
        );

        let stores = Arc::new(stores);

        let sync_manager = SyncManager::new(
            sender.clone(),
            stores.clone(),
            state.clone(),
            bus.clone(),
        );

        Self {
            state,
            bus,
            dispatcher,
            router,
            sync_manager,
            chain,
            transport,
            sender,
            stores,
            current_user_id,
            router_handle: None,
        }
    }

    /// 连接并启动所有子系统
    ///
    /// 内部会等待 CONNACK 到达后再返回，确保后续 `bootstrap()` 可安全发送请求。
    pub async fn connect(&mut self, user_id: &str, token: &str) -> Result<()> {
        if !self.state.transition(SdkState::Disconnected, SdkState::Connecting) {
            let cur = self.state.get();
            if cur == SdkState::Ready || cur == SdkState::Connected {
                return Ok(());
            }
            return Err(SdkError::InvalidState {
                expected: "Disconnected",
                actual: cur.to_string(),
            });
        }
        self.bus.publish(SdkEvent::StateChanged { state: SdkState::Connecting });

        let ready_notify = Arc::new(Notify::new());
        let handler = Arc::new(SocketHandler::new(
            self.dispatcher.clone(),
            Arc::new(ProtobufCodec),
            ready_notify.clone(),
        ));

        match self.transport.connect(user_id, token, handler, ready_notify).await {
            Ok(()) => {
                *self.current_user_id.write().await = user_id.to_string();
                self.state.set(SdkState::Connected);
                self.bus.publish(SdkEvent::StateChanged { state: SdkState::Connected });

                let handle = self.router.start(&self.bus);
                self.router_handle = Some(handle);

                info!(user_id, "engine connected");
                Ok(())
            }
            Err(e) => {
                self.state.reset();
                self.bus.publish(SdkEvent::StateChanged { state: SdkState::Disconnected });
                Err(e)
            }
        }
    }

    /// 执行 Bootstrap 同步
    pub async fn bootstrap(&mut self) -> Result<()> {
        self.sync_manager.bootstrap().await
    }

    /// 断开连接
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(h) = self.router_handle.take() {
            h.abort();
        }
        self.transport.disconnect().await?;
        *self.current_user_id.write().await = String::new();
        self.state.reset();
        self.bus.publish(SdkEvent::StateChanged { state: SdkState::Disconnected });
        info!("engine disconnected");
        Ok(())
    }

    pub fn state(&self) -> SdkState {
        self.state.get()
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn sender(&self) -> &Arc<PacketSender> {
        &self.sender
    }

    pub fn stores(&self) -> &Arc<StoreProvider> {
        &self.stores
    }

    pub fn sync_manager(&self) -> &SyncManager {
        &self.sync_manager
    }

    pub fn sync_manager_mut(&mut self) -> &mut SyncManager {
        &mut self.sync_manager
    }
}
