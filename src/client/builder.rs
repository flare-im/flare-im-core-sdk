use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::{MessageApi, ConversationApi};
use crate::client::config::SdkConfig;
use crate::client::im_client::IMClient;
use crate::core::{CurrentUserIdStore, SdkEngine};
use crate::middleware::{MiddlewareChain, MessageInterceptor, EventInterceptor};
use crate::protocol::{Codec, ProtobufCodec};
use crate::store::StoreProvider;
use crate::sync::SyncTask;
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
    pub fn add_message_interceptor(mut self, interceptor: impl MessageInterceptor + 'static) -> Self {
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
        let transport = SocketTransport::with_codec(self.config.clone(), codec);

        let mut chain = MiddlewareChain::new();
        for i in self.message_interceptors { chain.add_message_interceptor(i); }
        for i in self.event_interceptors { chain.add_event_interceptor(i); }

        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new(String::new()));
        let mut engine = SdkEngine::new(stores, chain, transport, current_user_id.clone());

        for task in self.sync_tasks {
            engine.sync_manager_mut().register_task_arc(task);
        }

        let sender = engine.sender().clone();
        let store_ref = engine.stores().clone();
        let chain_ref = Arc::new(MiddlewareChain::new());

        let message_api = MessageApi::new(
            sender.clone(),
            store_ref.messages.clone(),
            chain_ref,
            current_user_id.clone(),
        );
        let conversation_api = ConversationApi::new(store_ref.conversations.clone(), current_user_id);

        IMClient {
            engine,
            message_api,
            conversation_api,
        }
    }
}

impl Default for IMClientBuilder {
    fn default() -> Self { Self::new() }
}
