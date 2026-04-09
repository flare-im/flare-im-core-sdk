use std::sync::Arc;

use tokio::sync::RwLock;

use crate::application::event_deduper::EventDeduper;
use crate::application::message_deduper::MessageDeduper;
use crate::application::sync_task::{
    ConversationsSyncTask, KeyEventsSyncTask, MessagesSyncTask, ReadStatesSyncTask,
};
use crate::application::usecases::{
    ConversationCommandUseCase, ConversationViewAssembler, MessageMutationUseCase,
    MessageSendUseCase, MessageViewAssembler,
};
use crate::application::{MediaService, SyncProtocolAdapter};
use crate::client::config::SdkConfig;
use crate::client::im_client::{IMClient, IMClientInner};
use crate::core::{
    CurrentUserIdStore, SdkEngine, SessionSyncRunner, SyncResponseHandler, SyncTask,
};
use crate::event::EventBus;
use crate::client::api::{ConversationApi, MediaApi, MessageApi, MessageBuildApi};
use crate::middleware::{EventInterceptor, MessageInterceptor, MiddlewareChain};
use crate::protocol::{Codec, ProtobufCodec};
use crate::store::StoreProvider;
use crate::transport::{HttpClient, HttpRequestContext, SocketTransport};

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
    /// 创建构建器，带默认 SDK 配置。
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

    /// 设置 SDK 基础配置（网络、超时、重连等）。
    pub fn config(mut self, config: SdkConfig) -> Self {
        self.config = config;
        self
    }

    /// 注入存储实现（必填）。
    ///
    /// 未设置时 [`Self::build`] 会 panic。
    pub fn stores(mut self, stores: StoreProvider) -> Self {
        self.stores = Some(stores);
        self
    }

    /// 设置编解码器实现；未设置时默认使用 `ProtobufCodec`。
    pub fn codec(mut self, codec: Arc<dyn Codec>) -> Self {
        self.codec = Some(codec);
        self
    }

    /// 注册自定义同步任务（支持同步/异步完成）
    pub fn add_sync_task(mut self, task: impl SyncTask + 'static) -> Self {
        self.sync_tasks.push(Arc::new(task));
        self
    }

    /// 注册已装箱的同步任务实现。
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

    /// 构建 [`IMClient`] 并装配引擎、API 门面和默认同步任务。
    ///
    /// 该方法只完成组装，不会自动登录或建连。
    pub fn build(self) -> IMClient {
        let stores = self.stores.expect("StoreProvider is required");

        let codec: Arc<dyn Codec> = self.codec.unwrap_or_else(|| Arc::new(ProtobufCodec));
        let transport = SocketTransport::with_codec(self.config.clone(), codec.clone());
        let sender = transport.sender();
        let bus = EventBus::new();
        let event_deduper = EventDeduper::new(None);
        let message_deduper = MessageDeduper::new(None);
        let sync_handler: Arc<SyncProtocolAdapter> = Arc::new(SyncProtocolAdapter::new(
            sender.clone(),
            stores.clone(),
            bus.clone(),
            event_deduper.clone(),
            message_deduper.clone(),
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
            event_deduper,
            message_deduper,
        );

        // 注入应用层同步任务（构造时传入 SyncProtocolAdapter，execute 内自行调用，与用户扩展一致）
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
        sync_handler.set_reliable_queue(reliable_queue.clone());
        let bus = engine.bus().clone();

        let message_build_api = Arc::new(MessageBuildApi::new(
            current_user_id.clone(),
            store_ref.conversations.clone(),
        ));
        let media_base_url = self
            .config
            .http_url
            .clone()
            .unwrap_or_else(|| "http://localhost:50050".to_string());
        let http_request_context = Arc::new(HttpRequestContext::new());
        let media_service = Arc::new(MediaService::new(
            HttpClient::with_context(media_base_url, http_request_context.clone()),
            current_user_id.clone(),
            store_ref.upload_manifest_store.clone(),
            store_ref.media_cache_store.clone(),
            store_ref.media_cache_admin.clone(),
            store_ref.user_file_download_store.clone(),
        ));
        let message_send_use_case = Arc::new(MessageSendUseCase::new(
            sender.clone(),
            store_ref.messages.clone(),
            chain_ref,
            current_user_id.clone(),
            reliable_queue,
            media_service.clone(),
        ));
        let message_mutation_use_case = Arc::new(MessageMutationUseCase::new(
            sender,
            store_ref.messages.clone(),
            current_user_id.clone(),
            Some(bus.clone()),
        ));
        let message_view_assembler = Arc::new(MessageViewAssembler::new(
            store_ref.messages.clone(),
            profile_reader.clone(),
        ));
        let media_api = Arc::new(MediaApi::from_handler(media_service));
        let conversation_command_use_case = Arc::new(ConversationCommandUseCase::new(
            store_ref.conversations.clone(),
            current_user_id,
        ));
        let conversation_view_assembler = Arc::new(ConversationViewAssembler::new(
            store_ref.conversations.clone(),
            profile_reader,
        ));

        let conversation_api = Arc::new(ConversationApi::new(
            conversation_command_use_case,
            conversation_view_assembler,
            bus.clone(),
        ));

        IMClient::from_inner(IMClientInner {
            engine: Some(engine),
            message_api: Some(MessageApi::new(
                message_send_use_case,
                message_mutation_use_case,
                message_view_assembler,
            )),
            media_api: Some(media_api),
            message_build_api: Some(message_build_api),
            conversation_api: Some(conversation_api),
            http_request_context: Some(http_request_context),
            ..Default::default()
        })
    }
}

impl Default for IMClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
