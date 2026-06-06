use std::sync::Arc;

use tokio::sync::RwLock;

use crate::application::SyncProtocolAdapter;
use crate::application::notification::{NotificationHandlerRegistry, NotificationInboundPipeline};
use crate::application::services::EventDeduper;
use crate::application::services::MessageDeduper;
use crate::application::sync_task::{
    ConversationSettingsSyncTask, ConversationsSyncTask, KeyEventsSyncTask, MessagesSyncTask,
    ReadStatesSyncTask,
};
use crate::application::usecases::{
    ConversationCommandUseCase, ConversationViewAssembler, MessageMutationUseCase,
    MessageSendUseCase, MessageViewAssembler,
};
use crate::client::api::{
    CapabilityApi, ConversationApi, MediaApi, MessageApi, MessageBuildApi, PresenceApi,
};
use crate::client::config::SdkConfig;
use crate::client::im_client::{IMClient, IMClientInner};
use crate::core::event::EventBus;
use crate::core::{
    ConversationSummarySync, CurrentUserIdStore, SdkEngine, SessionSyncRunner, SyncResponseHandler,
    SyncTask,
};
use crate::extension::capability::AvCapabilityPlugin;
use crate::extension::capability::{
    SdkCapabilityPlugin, SdkCapabilityRegistry, reserved_namespaces_of_plugin,
};
use crate::extension::middleware::{EventInterceptor, MessageInterceptor, MiddlewareChain};
use crate::extension::{ExtensionRegistry, SdkExtension};
use crate::infrastructure::persistence::StoreProvider;
use crate::infrastructure::protocol::{Codec, ProtobufCodec};
use crate::infrastructure::transport::{HttpClient, HttpRequestContext, SocketTransport};
use crate::platform::adapters::media::{MediaService, UploadOnlyMediaService};
use crate::platform::ports::media::MediaServicePort;
use crate::platform::runtime::RuntimeComponents;
use crate::shared::error::{ErrorCode, FlareError, Result};
use flare_proto::common::CallSignalEvent;

#[cfg(not(target_arch = "wasm32"))]
fn derive_capability_url(config: &SdkConfig) -> String {
    if let Ok(url) = std::env::var("FLARE_CAPABILITY_GRPC_URI")
        .or_else(|_| std::env::var("FLARE_IM_CAPABILITY_GRPC_URI"))
    {
        let url = url.trim();
        if !url.is_empty() {
            return url.to_string();
        }
    }
    if let Some(url) = config
        .capability_url
        .as_ref()
        .filter(|u| !u.trim().is_empty())
    {
        return url.clone();
    }
    if let Some(http_url) = config.http_url.as_ref()
        && let Ok(mut parsed) = url::Url::parse(http_url)
    {
        let _ = parsed.set_port(Some(50110));
        parsed.set_path("");
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.to_string().trim_end_matches('/').to_string();
    }
    "http://localhost:50110".to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn derive_online_url(config: &SdkConfig) -> String {
    if let Some(url) = config.online_url.as_ref().filter(|u| !u.trim().is_empty()) {
        return url.clone();
    }
    if let Some(http_url) = config.http_url.as_ref()
        && let Ok(mut parsed) = url::Url::parse(http_url)
    {
        let _ = parsed.set_port(Some(50061));
        parsed.set_path("");
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.to_string().trim_end_matches('/').to_string();
    }
    "http://localhost:50061".to_string()
}

#[cfg(target_arch = "wasm32")]
fn derive_capability_url(config: &SdkConfig) -> String {
    config
        .capability_url
        .clone()
        .unwrap_or_else(|| "http://localhost:50110".to_string())
}

#[cfg(target_arch = "wasm32")]
fn derive_online_url(config: &SdkConfig) -> String {
    config
        .online_url
        .clone()
        .unwrap_or_else(|| "http://localhost:50061".to_string())
}

/// IMClient 构建器
///
/// 核心只构建消息和会话能力，其他功能通过扩展注入：
/// - `add_sync_task`: 注册自定义同步任务（业务 SDK 扩展，如社交好友/群同步）
/// - `add_message_interceptor`: 消息拦截（端到端加密、内容过滤等）
/// - `add_event_interceptor`: 事件拦截（日志、审计等）
///
/// ```ignore
/// let client = IMClient::builder()
///     .config(SdkConfig::new("wss://im.example.com"))
///     .stores(stores)
///     .add_sync_task(MyBusinessSync::new())    // 扩展：业务同步任务
///     .add_message_interceptor(E2EEncryption::new()) // 扩展：端到端加密
///     .build()?;
/// ```
pub struct IMClientBuilder {
    config: SdkConfig,
    stores: Option<StoreProvider>,
    runtime: Option<RuntimeComponents>,
    http_request_context: Option<Arc<HttpRequestContext>>,
    codec: Option<Arc<dyn Codec>>,
    sync_tasks: Vec<Arc<dyn SyncTask>>,
    message_interceptors: Vec<Arc<dyn MessageInterceptor>>,
    event_interceptors: Vec<Arc<dyn EventInterceptor>>,
    capability_plugins: Vec<Arc<dyn SdkCapabilityPlugin>>,
    allow_reserved_capability_namespace_override: bool,
    builder_errors: Vec<FlareError>,
}

impl IMClientBuilder {
    /// 创建构建器，带默认 SDK 配置。
    pub fn new() -> Self {
        Self {
            config: SdkConfig::default(),
            stores: None,
            runtime: None,
            http_request_context: None,
            codec: None,
            sync_tasks: Vec::new(),
            message_interceptors: Vec::new(),
            event_interceptors: Vec::new(),
            capability_plugins: Vec::new(),
            allow_reserved_capability_namespace_override: false,
            builder_errors: Vec::new(),
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

    /// 注入已装配好的运行时组件。
    ///
    /// 这是 Web/RN/uni-app/Electron/Android/iOS/鸿蒙等平台 adapter 的主接入点。
    /// 原生测试和服务端内嵌场景可直接使用 `.stores(...)` 注入内存或 SQLite 存储。
    pub fn runtime(mut self, runtime: RuntimeComponents) -> Self {
        self.runtime = Some(runtime);
        self
    }

    /// 注入共享 HTTP 鉴权上下文（与 Social Gateway / 媒体 HTTP 共用 Bearer 与链路头）。
    pub fn http_request_context(mut self, context: Arc<HttpRequestContext>) -> Self {
        self.http_request_context = Some(context);
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

    /// 注册自定义能力插件（开源核心与商业扩展统一入口）。
    ///
    /// 默认策略下，插件不得覆盖核心保留命名空间（如 `rtc`）；
    /// 若确需覆盖，请显式调用
    /// [`Self::allow_reserved_capability_namespace_override_for_private_distribution`]。
    pub fn add_capability_plugin(mut self, plugin: impl SdkCapabilityPlugin + 'static) -> Self {
        self.capability_plugins.push(Arc::new(plugin));
        self
    }

    /// 注册已装箱能力插件。
    pub fn add_capability_plugin_arc(mut self, plugin: Arc<dyn SdkCapabilityPlugin>) -> Self {
        self.capability_plugins.push(plugin);
        self
    }

    /// 安装业务扩展（如 flare-social-sdk）。
    ///
    /// 扩展只能注册任务、拦截器和能力插件；核心 IM API 不因业务扩展而变化。
    pub fn add_extension(mut self, extension: impl SdkExtension + 'static) -> Self {
        let mut registry = ExtensionRegistry::default();
        match extension.install(&mut registry) {
            Ok(()) => {
                self.sync_tasks.extend(registry.sync_tasks);
                self.message_interceptors
                    .extend(registry.message_interceptors);
                self.event_interceptors.extend(registry.event_interceptors);
                self.capability_plugins.extend(registry.capability_plugins);
            }
            Err(err) => {
                tracing::error!(
                    namespace = extension.namespace(),
                    error = %err,
                    "SDK extension installation failed"
                );
                self.builder_errors.push(err);
            }
        }
        self
    }

    /// 私有发行开关：允许外部插件覆盖核心保留命名空间。
    ///
    /// 开源默认应保持关闭，避免核心路由被意外替换。
    pub fn allow_reserved_capability_namespace_override_for_private_distribution(
        mut self,
        enabled: bool,
    ) -> Self {
        self.allow_reserved_capability_namespace_override = enabled;
        self
    }

    /// 构建 [`IMClient`] 并装配引擎、API 门面和默认同步任务。
    ///
    /// 该方法只完成组装，不会自动登录或建连。
    pub fn build(self) -> Result<IMClient> {
        if let Some(err) = self.builder_errors.first().cloned() {
            return Err(err);
        }
        let codec: Arc<dyn Codec> = self.codec.unwrap_or_else(|| Arc::new(ProtobufCodec));
        let (stores, transport, runtime_media_service) = match self.runtime {
            Some(RuntimeComponents {
                stores,
                transport,
                media_service,
                media_uploader,
                ..
            }) => {
                let media_service = media_service.or_else(|| {
                    media_uploader.map(|uploader| {
                        Arc::new(UploadOnlyMediaService::new(uploader)) as Arc<dyn MediaServicePort>
                    })
                });
                (stores, transport, media_service)
            }
            None => {
                let stores = self.stores.ok_or_else(|| {
                    FlareError::localized(
                        ErrorCode::ConfigurationError,
                        "StoreProvider is required",
                    )
                })?;
                let transport = SocketTransport::with_codec(self.config.clone(), codec.clone());
                (stores, transport, None)
            }
        };
        let sender = transport.sender();
        let mut chain = MiddlewareChain::new();
        for i in self.message_interceptors {
            chain.add_message_interceptor(i);
        }
        for i in self.event_interceptors {
            chain.add_event_interceptor(i);
        }
        let chain = Arc::new(chain);
        let bus = EventBus::with_middleware(chain.clone());
        let event_deduper = EventDeduper::new(None);
        let message_deduper = MessageDeduper::new(None);
        let notification_registry = Arc::new(NotificationHandlerRegistry::new());
        let notification_pipeline = NotificationInboundPipeline::new(
            notification_registry.clone(),
            message_deduper.clone(),
            bus.clone(),
        );
        let init_msg_concurrency = self.config.init_message_sync_concurrency() as usize;
        let sync_handler: Arc<SyncProtocolAdapter> = Arc::new(SyncProtocolAdapter::new(
            sender.clone(),
            stores.clone(),
            bus.clone(),
            event_deduper.clone(),
            notification_pipeline.clone(),
            init_msg_concurrency,
        ));

        let current_user_id: CurrentUserIdStore = Arc::new(RwLock::new(String::new()));
        let engine = SdkEngine::new(crate::core::SdkEngineConfig {
            stores,
            chain: chain.clone(),
            transport,
            current_user_id: current_user_id.clone(),
            codec,
            bus,
            sync_response_handler: Some(sync_handler.clone() as Arc<dyn SyncResponseHandler>),
            session_sync: Some(sync_handler.clone() as Arc<dyn SessionSyncRunner>),
            conversation_summary_sync: Some(
                sync_handler.clone() as Arc<dyn ConversationSummarySync>
            ),
            event_deduper,
            message_deduper,
            notification_pipeline,
        });

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
        engine
            .sync_manager()
            .register_task_arc(Arc::new(ConversationSettingsSyncTask::new(
                sync_handler.clone(),
            )));
        for task in self.sync_tasks {
            engine.sync_manager().register_task_arc(task);
        }

        let sender = engine.sender().clone();
        let store_ref = engine.stores().clone();
        let chain_ref = engine.middleware_chain();
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
        #[cfg(target_arch = "wasm32")]
        let media_http_base = media_base_url.clone();
        let capability_base_url = derive_capability_url(&self.config);
        #[cfg(not(target_arch = "wasm32"))]
        let online_base_url = derive_online_url(&self.config);
        #[cfg(target_arch = "wasm32")]
        let _online_base_url = derive_online_url(&self.config);
        let tenant_id = self
            .config
            .tenant_id
            .clone()
            .or_else(|| std::env::var("FLARE_IM_TENANT_ID").ok())
            .map(crate::shared::util::normalize_tenant_id)
            .unwrap_or_else(|| "0".to_string());
        let http_request_context = self
            .http_request_context
            .unwrap_or_else(|| Arc::new(HttpRequestContext::new()));
        let default_media_service: Arc<dyn MediaServicePort> = Arc::new(MediaService::new(
            HttpClient::with_context(media_base_url, http_request_context.clone()),
            current_user_id.clone(),
            store_ref.upload_manifest_store.clone(),
            store_ref.media_cache_store.clone(),
            store_ref.media_cache_admin.clone(),
            store_ref.user_file_download_store.clone(),
        ));
        let media_service = runtime_media_service.unwrap_or(default_media_service);
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
        let capability_api = Arc::new(CapabilityApi::new(
            capability_base_url,
            current_user_id.clone(),
            tenant_id.clone(),
            http_request_context.clone(),
        ));
        let presence_api = Arc::new(PresenceApi::new(
            {
                #[cfg(target_arch = "wasm32")]
                {
                    media_http_base
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    online_base_url
                }
            },
            current_user_id.clone(),
            tenant_id,
            http_request_context.clone(),
            bus.clone(),
        ));
        let capability_registry = Arc::new(SdkCapabilityRegistry::new());
        let _ =
            capability_registry.register(Arc::new(AvCapabilityPlugin::new(capability_api.clone())));
        capability_registry.register_call_signal_observer(|cid, _| {
            tracing::debug!(
                target = "flare_sdk.plugin.call",
                conversation_id = %cid,
                "call_signal (observer)"
            );
        });
        let reg = capability_registry.clone();
        let _call_signal_plugin_bridge =
            bus.on_call_signal(move |conversation_id: &str, event: &CallSignalEvent| {
                reg.dispatch_call_signal_to_observers(conversation_id, event);
            });
        for plugin in self.capability_plugins {
            let plugin_id = plugin.plugin_id();
            let reserved = reserved_namespaces_of_plugin(plugin.as_ref());
            let register_result = if reserved.is_empty() {
                capability_registry.register(plugin.clone())
            } else if self.allow_reserved_capability_namespace_override {
                tracing::warn!(
                    target = "flare_sdk.capability",
                    plugin_id = plugin_id,
                    namespaces = ?reserved,
                    "register plugin with reserved namespace override (private distribution)"
                );
                capability_registry.register_with_namespace_override(plugin.clone())
            } else {
                tracing::warn!(
                    target = "flare_sdk.capability",
                    plugin_id = plugin_id,
                    namespaces = ?reserved,
                    "skip plugin registration: reserved namespace conflict"
                );
                continue;
            };
            if let Err(err) = register_result {
                tracing::warn!(
                    target = "flare_sdk.capability",
                    plugin_id = plugin_id,
                    error = %err,
                    "register capability plugin failed"
                );
            }
        }
        let conversation_command_use_case = Arc::new(ConversationCommandUseCase::new(
            store_ref.conversations.clone(),
            current_user_id,
            Some(sync_handler.clone()),
            Some(store_ref.cursors.clone()),
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

        Ok(IMClient::from_inner(IMClientInner {
            engine: Some(engine),
            message_api: Some(MessageApi::new(
                message_send_use_case,
                message_mutation_use_case,
                message_view_assembler,
            )),
            media_api: Some(media_api),
            capability_api: Some(capability_api),
            presence_api: Some(presence_api),
            capability_registry: Some(capability_registry),
            message_build_api: Some(message_build_api),
            conversation_api: Some(conversation_api),
            http_request_context: Some(http_request_context),
            notification_registry: Some(notification_registry),
            ..Default::default()
        }))
    }
}

impl Default for IMClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
