use crate::api::{MessageApi, ConversationApi};
use crate::client::builder::IMClientBuilder;
use crate::core::{SdkEngine, SdkState};
use crate::error::Result;
use crate::event::{EventBus, SharedEvent, Subscription};

/// Flare IM 客户端 — SDK 唯一入口
///
/// 核心只提供消息和会话能力，其他功能（用户/群组/在线状态等）
/// 通过 SyncTask + EventInterceptor + Extension 事件进行扩展。
///
/// # 基础用法
///
/// ```ignore
/// let client = IMClient::builder()
///     .config(SdkConfig::new("wss://im.example.com"))
///     .stores(stores)
///     .build();
///
/// client.connect("user_123", "jwt_token").await?;
///
/// // 发送消息
/// let ack = client.message().send(msg).await?;
///
/// // 查询会话
/// let conversations = client.conversation().list().await?;
///
/// // 监听消息
/// let _sub = client.on_message(|msg| {
///     println!("new message: {}", msg.server_id);
/// });
///
/// // 监听扩展事件（如在线状态）
/// let _sub = client.on(|e| {
///     if let SdkEvent::Extension { source, event_type, payload } = &*e {
///         // 处理扩展事件
///     }
/// });
///
/// // 同步
/// client.sync_conversation("conv_id").await?;
///
/// // 断开
/// client.disconnect().await?;
/// ```
pub struct IMClient {
    pub(crate) engine: SdkEngine,
    pub(crate) message_api: MessageApi,
    pub(crate) conversation_api: ConversationApi,
}

impl IMClient {
    pub fn builder() -> IMClientBuilder {
        IMClientBuilder::new()
    }

    // ── 生命周期 ────────────────────────────────────────────

    /// 连接服务器 + 启动后台子系统
    pub async fn connect(&mut self, user_id: &str, token: &str) -> Result<()> {
        self.engine.connect(user_id, token).await?;
        self.engine.bootstrap().await?;
        Ok(())
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        self.engine.disconnect().await
    }

    pub fn state(&self) -> SdkState {
        self.engine.state()
    }

    // ── 核心 API（消息 + 会话）────────────────────────────

    pub fn message(&self) -> &MessageApi {
        &self.message_api
    }

    pub fn conversation(&self) -> &ConversationApi {
        &self.conversation_api
    }

    // ── 事件 ────────────────────────────────────────────────

    pub fn bus(&self) -> &EventBus {
        self.engine.bus()
    }

    /// 注册通用事件回调（含 Extension 扩展事件）
    pub fn on<F>(&self, callback: F) -> Subscription
    where F: Fn(SharedEvent) + Send + Sync + 'static {
        self.engine.bus().on(callback)
    }

    /// 注册消息接收回调
    pub fn on_message<F>(&self, callback: F) -> Subscription
    where F: Fn(&flare_proto::common::Message) + Send + Sync + 'static {
        self.engine.bus().on_message(callback)
    }

    /// 注册状态变更回调
    pub fn on_state_changed<F>(&self, callback: F) -> Subscription
    where F: Fn(SdkState) + Send + Sync + 'static {
        self.engine.bus().on_state_changed(callback)
    }

    // ── 同步 ────────────────────────────────────────────────

    /// 重新全量同步会话列表（从服务端拉取最新会话 → 写入本地 Store）
    pub async fn sync_conversations(&self) -> Result<()> {
        self.engine.sync_manager().sync_conversations().await
    }

    /// 增量同步单个会话
    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        self.engine.sync_manager().sync_conversation(conversation_id).await
    }

    /// 标记会话已读：同时更新消息已读回执与会话未读数（统一逻辑，多端复用；user_id 从 SDK 当前用户获取）
    pub async fn mark_session_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        self.message_api.mark_read(conversation_id, read_seq).await?;
        self.conversation_api
            .mark_read(conversation_id, read_seq)
            .await
    }

    // ── 引擎访问（供扩展使用）────────────────────────────

    pub fn engine(&self) -> &SdkEngine {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut SdkEngine {
        &mut self.engine
    }
}
