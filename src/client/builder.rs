use std::sync::Arc;

use tokio::sync::RwLock;

use crate::application::handlers::SyncHandler;
use crate::application::sync_task::{
    ConversationsSyncTask, KeyEventsSyncTask, MessagesSyncTask, ReadStatesSyncTask,
};
use crate::application::{
    ConversationFlow, ConversationQueryHandler, MessageEngine, MessageQueryHandler,
};
use crate::client::config::SdkConfig;
use crate::client::im_client::{IMClient, IMClientInner};
use crate::core::{
    CurrentUserIdStore, SdkEngine, SessionSyncRunner, SyncResponseHandler, SyncTask,
};
use crate::event::EventBus;
use crate::client::api::{ConversationApi, MessageApi, MessageBuildApi};
use crate::middleware::{EventInterceptor, MessageInterceptor, MiddlewareChain};
use crate::protocol::{Codec, ProtobufCodec};
use crate::store::StoreProvider;
use crate::transport::SocketTransport;

/// IMClient 构建器
///
/// 核心只构建消息和会话能力，其他功能通过扩展注入：
/// - `add_sync_task`: 注册自定义同步任务（联系人、群列表等）
/// - `add_message_interceptor`: 消息拦截（端到端加密、内容过滤等）
/// - `add_event_interceptor`: 事件拦截（日志、审计等）
///
/// ```ignore
/// let client = IMClient::builder()
///     .config(SdkConfig::new("wss://im.example.com"))
///     .stores(stores)
///     .add_sync_task(ContactSync::new())       // 扩展：联系人同步
///     .add_message_interceptor(E2EEncryption::new()) // 扩展：端到端加密
///     .build();
/// ```
pub struct IMClientBuilder {
    config: SdkConfig,
    stores: Option<StoreProvider>,
    codec: Option<Arc<dyn Codec>>,
    sync_tasks: Vec<Arc<dyn SyncTask>>,
    message_interceptors: Vec<Arc<dyn MessageInterceptor>>,
    event_interceptors: Vec<Arc<dyn EventInterceptor>>,
}

impl IMClientBuilder {
    pub fn new() -> Self {
        Self {
            config: SdkConfig::default(),
            stores: None,
            codec: None,
            sync_tasks: Vec::new(),
            message_interceptors: Vec::new(),
            event_interceptors: Vec::new(),
        }
    }

    pub fn config(mut self, config: SdkConfig) -> Self {
        self.config = config;
        self
    }

    pub fn stores(mut self, stores: StoreProvider) -> Self {
        self.stores = Some(stores);
        self
    }

    pub fn codec(mut self, codec: Arc<dyn Codec>) -> Self {
        self.codec = Some(codec);
        self
    }

    /// 注册自定义同步任务（支持同步/异步完成）
    pub fn add_sync_task(mut self, task: impl SyncTask + 'static) -> Self {
        self.sync_tasks.push(Arc::new(task));
        self
    }

    pub fn add_sync_task_arc(mut self, task: Arc<dyn SyncTask>) -> Self {
        self.sync_tasks.push(task);
        self
    }

    /// 注册消息拦截器（加密/过滤/富化）
    pub fn add_message_interceptor(
        mut self,
        interceptor: impl MessageInterceptor + 'static,
    ) -> Self {
        self.message_interceptors.push(Arc::new(interceptor));
        self
    }

    /// 注册事件拦截器
    pub fn add_event_interceptor(mut self, interceptor: impl EventInterceptor + 'static) -> Self {
        self.event_interceptors.push(Arc::new(interceptor));
        self
    }

    pub fn build(self) -> IMClient {
        let stores = self.stores.expect("StoreProvider is required");

        let codec: Arc<dyn Codec> = self.codec.unwrap_or_else(|| Arc::new(ProtobufCodec));
        let transport = SocketTransport::with_codec(self.config.clone(), codec.clone());
        let sender = transport.sender();
        let bus = EventBus::new();
        let sync_handler: Arc<SyncHandler> = Arc::new(SyncHandler::new(
            sender.clone(),
            stores.clone(),
            bus.clone(),
        ));

        let mut chain = MiddlewareChain::new();
        for i in self.message_interceptors {
            chain.add_message_interceptor(i);
        }
        for i in self.event_interceptors {
            chain.add_event_interceptor(i);
        }

        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new(String::new()));
        let engine = SdkEngine::new(
            stores,
            chain,
            transport,
            current_user_id.clone(),
            codec,
            bus,
            Some(sync_handler.clone() as Arc<dyn SyncResponseHandler>),
            Some(sync_handler.clone() as Arc<dyn SessionSyncRunner>),
        );

        // 注入应用层同步任务（构造时传入 SyncHandler，execute 内自行调用，与用户扩展一致）
        engine
            .sync_manager()
            .register_task_arc(Arc::new(ConversationsSyncTask::new(sync_handler.clone())));
        engine
            .sync_manager()
            .register_task_arc(Arc::new(MessagesSyncTask::new(sync_handler.clone())));
        engine
            .sync_manager()
            .register_task_arc(Arc::new(KeyEventsSyncTask::new(sync_handler.clone())));
        engine
            .sync_manager()
            .register_task_arc(Arc::new(ReadStatesSyncTask::new(sync_handler.clone())));
        for task in self.sync_tasks {
            engine.sync_manager().register_task_arc(task);
        }

        let sender = engine.sender().clone();
        let store_ref = engine.stores().clone();
        let chain_ref = Arc::new(MiddlewareChain::new());
        let profile_reader = store_ref.user_profiles_or_memory();
        let reliable_queue = engine.reliable_queue();
        let bus = engine.bus().clone();

        let conversation_query_handler = Arc::new(ConversationQueryHandler::new(
            store_ref.conversations.clone(),
        ));
        let message_query_handler = Arc::new(MessageQueryHandler::new(store_ref.messages.clone()));

        let message_engine = Arc::new(MessageEngine::new(
            sender,
            store_ref.messages.clone(),
            message_query_handler,
            chain_ref,
            current_user_id.clone(),
            profile_reader.clone(),
            reliable_queue,
            Some(bus.clone()),
        ));
        let message_build_api = Arc::new(MessageBuildApi::new(
            current_user_id.clone(),
            conversation_query_handler.clone(),
        ));
        let conversation_flow = Arc::new(ConversationFlow::new(
            store_ref.conversations.clone(),
            conversation_query_handler,
            current_user_id,
            profile_reader,
        ));

        let conversation_api = Arc::new(ConversationApi::new(conversation_flow, bus.clone()));

        IMClient::from_inner(IMClientInner {
            engine: Some(engine),
            message_api: Some(MessageApi::new(message_engine)),
            message_build_api: Some(message_build_api),
            conversation_api: Some(conversation_api),
            ..Default::default()
        })
    }
}

impl Default for IMClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
