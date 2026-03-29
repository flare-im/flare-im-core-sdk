//! [`IMClient`] 事件订阅：一律委托 [`crate::event::EventBus`]，与消息/会话领域对齐。
//!
//! 专业 SDK 常见形态是「单入口 + 类型化 on_*」；实现保持薄转发即可，不必按域拆多个源文件。
//!
//! **注意**：若在 [`super::IMClient::login`] 的 `before_connect` 之前尚未挂上引擎，应优先在回调里用传入的 [`crate::event::EventBus`] 注册；连接后再用本文件的 `on_*` 亦可（见示例 `two_clients_chat`）。

use flare_proto::common::{MessageRecallEvent, SendAck, TypingEvent};

use crate::client::IMClient;
use crate::core::SdkState;
use crate::event::{SharedEvent, Subscription, SyncPhase};
use crate::fsm::SyncState;
use crate::model::IMMessage;
use crate::Result;

impl IMClient {
    // ========== 连接 / 会话状态机（[`SdkState`]）==========

    /// 与服务端 WebSocket（或等价链路）**建连成功**后触发；可做 UI 切到「已连接」。
    pub fn on_connected<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_connected(f))
    }

    /// **断线**时触发；参数为断开原因说明字符串（如服务端关闭、网络错误文案）。
    pub fn on_disconnected<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_disconnected(f))
    }

    /// SDK 顶层状态变化（如 Disconnected → Connecting → Ready）；用于驱动全局连接指示、禁用发送按钮等。
    pub fn on_state_changed<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(SdkState) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_state_changed(f))
    }

    /// 服务端返回的业务/信令错误（`code` + `message`）；非单纯网络断开，可提示用户或打点。
    pub fn on_server_error<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(i32, &str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_server_error(f))
    }

    /// **被踢下线**（其他端登录、后台策略等）；参数多为服务端附带说明，应引导重新登录。
    pub fn on_kicked_off<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_kicked_off(f))
    }

    /// **Token 过期或即将不可用**；应刷新 JWT 后重连或调 [`super::IMClient::connect`]。
    pub fn on_token_expired<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_token_expired(f))
    }

    // ========== 同步引擎（会话列表 / 消息游标等任务）==========

    /// 同步状态机子状态变化（[`SyncState`]）；细粒度展示「正在同步会话 / 消息」等。
    pub fn on_sync_state_changed<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(SyncState) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_sync_state_changed(f))
    }

    /// 本轮同步任务**开始**（可能多次：冷启动、前后台切换等）。
    pub fn on_sync_started<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_sync_started(f))
    }

    /// 某一同步阶段**结束**；[`SyncPhase`] 区分前台全量、后台增量等，常用于「首屏同步完成」门禁。
    pub fn on_sync_finished<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(SyncPhase) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_sync_finished(f))
    }

    /// 同步任务**失败**；参数一般为 `(任务标识, 错误描述)`，可重试或上报。
    pub fn on_sync_failed<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(String, String) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_sync_failed(f))
    }

    /// 同步**进度**；`(任务标识, 0.0~1.0 进度, 附加上下文文案)`，用于进度条或日志。
    pub fn on_sync_progress<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(String, f32, String) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_sync_progress(f))
    }

    /// 单个同步子任务**完成**（如某一类数据拉取结束）；参数为任务名/类型字符串。
    pub fn on_sync_task_completed<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_sync_task_completed(f))
    }

    // ========== 消息（单条 / 批量 / 发送态 / 撤回 / 输入中）==========

    /// 收到**一条新消息**（推送或同步落库后）；单聊/群聊共用，内容为 [`IMMessage`]。
    pub fn on_message<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&IMMessage) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_message(f))
    }

    /// 批量新消息（如同步一批历史）；比逐条 `on_message` 更高效，适合列表差量刷新。
    pub fn on_message_batch<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&[IMMessage]) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_message_batch(f))
    }

    /// 发送**成功回执**（服务端 ACK，含 `seq` / `server_msg_id` 等）；用于把本地发送中状态改为已送达。
    pub fn on_send_ack<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&SendAck) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_send_ack(f))
    }

    /// 发送**失败**；参数为 `(client_msg_id, 失败原因)`，用于气泡显示重试或失败态。
    pub fn on_send_failed<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str, &str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_send_failed(f))
    }

    /// 消息被**撤回**；参数为 `(conversation_id, 撤回事件体)`，需更新本地消息列表展示。
    pub fn on_recalled<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str, &MessageRecallEvent) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_recalled(f))
    }

    /// **正在输入**；参数为 `(conversation_id, Typing 事件)`，用于展示「对方正在输入…」。
    pub fn on_typing<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str, &TypingEvent) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_typing(f))
    }

    // ========== 会话列表（元数据 / 未读 / 删除）==========

    /// 会话列表**同步完成**（或全量刷新）；参数为本次涉及的 `conversation_id` 列表。
    pub fn on_conversation_synced<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&[String]) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_conversation_synced(f))
    }

    /// **新建会话**（本地或远端）；参数为 `conversation_id`。
    pub fn on_conversation_created<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_conversation_created(f))
    }

    /// 会话**属性更新**（置顶、静音、草稿等）；参数为 `conversation_id`，可再拉取详情（如 `ConversationApi::get`）。
    pub fn on_conversation_updated<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_conversation_updated(f))
    }

    /// **未读数变化**；参数为 `(conversation_id, unread_count)`，用于角标与列表红点。
    pub fn on_conversation_unread_count_changed<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str, u32) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_conversation_unread_count_changed(f))
    }

    /// 会话**删除**（本地或同步删除）；参数为 `conversation_id`。
    pub fn on_conversation_deleted<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_conversation_deleted(f))
    }

    // ========== 扩展推送 / 通配监听 ==========

    /// **自定义扩展**下行；`(业务类型, 子类型, payload 字节)`，与核心消息并行，供音视频信令、运营活动等。
    pub fn on_extension<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(&str, &str, &[u8]) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_extension(f))
    }

    /// **所有**领域事件的通配入口（[`SharedEvent`] = `Arc<SdkEvent>`）；适合日志、调试或统一路由；生产环境注意性能与过滤。
    pub fn on_any<F>(&self, f: F) -> Result<Subscription>
    where
        F: Fn(SharedEvent) + Send + Sync + 'static,
    {
        self.with_engine(|e| e.bus().on_any(f))
    }
}
