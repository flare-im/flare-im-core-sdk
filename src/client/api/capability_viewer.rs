//! 插件卡片本人层（message-content 贡献的 `viewerState`）的后台读取。
//!
//! 宿主呈现卡片时（[`ViewerStateDriver::on_present`]）和卡片的状态层变了时（`PluginStateChanged`，
//! 来自共享状态事件、同步、补读或动作结果）按 [`ViewerSchedule`] 判定要不要读；要读就在后台用卡片
//! 载荷叠公开层绑定输入、以卡片消息的 server id 为 `messageId` 调这个 `query` 操作，把结果叠加成本人层
//! （`UiViewerState::local_layer`，带 `basedOnVersion`）按版本写回这条消息，写入了就发
//! `PluginStateChanged`，宿主照常重新呈现。Kit 与 app 不写任何调用逻辑。
//!
//! 预算：只给呈现过的卡片读；每张卡片同时一个请求、按 `minIntervalMs` 节流、失败指数退避；所有卡片
//! 合计同时最多 [`MAX_PARALLEL_READS`] 个请求；换登录用户后旧会话的结果一律不写。
use std::sync::{Arc, Mutex, PoisonError};

use flare_plugin_protocol::manifest::UiContribution;
use flare_plugin_protocol::ui::{PluginMessage, UiViewerState};
use tokio::sync::{RwLock, Semaphore};

use super::CapabilityApi;
use crate::application::services::plugin_viewer_schedule::{
    ViewerCard, ViewerDecision, ViewerSchedule, ViewerTrigger,
};
use crate::domain::{MessageStore, OperationApplyResult};
use crate::extension::capability::plugin_errors;
use crate::extension::capability::ui::CardState;
use crate::kernel::event::{
    EventBus, MessageEvent, MessageEventType, SdkEvent, SdkEventType, Subscription,
};
use crate::model::IMMessage;
use crate::shared::util::now_millis;

/// 所有卡片合计同时在途的本人层读取。
pub(crate) const MAX_PARALLEL_READS: usize = 4;

pub(crate) struct ViewerStateDriver {
    /// 发请求用；它自己不带驱动，不会反过来调度。
    api: CapabilityApi,
    messages: Arc<dyn MessageStore>,
    bus: EventBus,
    current_user: Arc<RwLock<String>>,
    schedule: Mutex<ViewerSchedule>,
    permits: Semaphore,
    subscription: Mutex<Option<Subscription>>,
}

/// 一张卡片此刻生效的本人层与公开层版本（没有时 0）。
fn layers_of(
    card_message: &PluginMessage,
    contribution: &UiContribution,
    stored: &IMMessage,
) -> (Option<flare_plugin_protocol::ui::PluginMessageState>, u64) {
    let entity = contribution.entity_of(&card_message.payload);
    let state = CardState::of(Some(stored), entity.as_deref());
    let shared = state
        .layers()
        .applied_shared(contribution, &card_message.payload)
        .map_or(0, |shared| shared.version);
    (state.local, shared)
}

impl ViewerStateDriver {
    pub(crate) fn new(
        api: CapabilityApi,
        messages: Arc<dyn MessageStore>,
        bus: EventBus,
        current_user: Arc<RwLock<String>>,
    ) -> Arc<Self> {
        let driver = Arc::new(Self {
            api,
            messages,
            bus,
            current_user,
            schedule: Mutex::new(ViewerSchedule::default()),
            permits: Semaphore::new(MAX_PARALLEL_READS),
            subscription: Mutex::new(None),
        });
        let weak = Arc::downgrade(&driver);
        let subscription = driver.bus.on_event_type(
            SdkEventType::Message(MessageEventType::PluginStateChanged),
            move |event| {
                let Some(driver) = weak.upgrade() else {
                    return;
                };
                if let SdkEvent::Message(MessageEvent::PluginStateChanged { message_id, .. }) =
                    event.as_ref()
                {
                    driver.on_state_changed(message_id);
                }
            },
        );
        *driver
            .subscription
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(subscription);
        driver
    }

    fn schedule(&self) -> std::sync::MutexGuard<'_, ViewerSchedule> {
        self.schedule.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn user(&self) -> String {
        self.current_user.read().await.trim().to_owned()
    }

    /// 宿主呈现了 `stored` 这条消息上的卡片：记下它（目录变了时据此重新宣告），`interactive`（可用、
    /// 不是未绑定副本）且 `contribution` 声明了 `viewerState` 时按需安排一次读取，立即返回。没有
    /// server id 的消息（还没落到服务端）不记。
    pub(crate) fn on_present(
        self: &Arc<Self>,
        stored: &IMMessage,
        contribution: Option<&UiContribution>,
        user: &str,
        interactive: bool,
    ) {
        let message_id = stored.server_id.trim();
        if message_id.is_empty() || stored.conversation_id.trim().is_empty() || user.is_empty() {
            return;
        }
        let Some(message) = stored.plugin_message() else {
            return;
        };
        let layers = contribution.map(|contribution| layers_of(&message, contribution, stored));
        let now = now_millis();
        let decision = {
            let mut schedule = self.schedule();
            schedule.presented(
                user,
                ViewerCard {
                    message_id: message_id.to_owned(),
                    conversation_id: stored.conversation_id.clone(),
                    message,
                    contribution: contribution.filter(|_| interactive).cloned(),
                },
                now,
            );
            match layers {
                Some((local, shared_version)) if interactive => schedule.decide(
                    user,
                    message_id,
                    ViewerTrigger::Present,
                    local.as_ref(),
                    shared_version,
                    now,
                ),
                _ => ViewerDecision::Skip,
            }
        };
        self.act(user.to_owned(), message_id.to_owned(), decision);
    }

    /// 某条消息的插件状态层变了（任何来源）。只对记着的卡片判定。
    fn on_state_changed(self: &Arc<Self>, message_id: &str) {
        if !self.schedule().tracks(message_id) {
            return;
        }
        let this = self.clone();
        let message_id = message_id.to_owned();
        crate::shared::util::background::spawn_background(async move {
            this.judge(message_id, ViewerTrigger::SharedVersion).await;
        });
    }

    async fn judge(self: Arc<Self>, message_id: String, trigger: ViewerTrigger) {
        let user = self.user().await;
        let Some(card) = self.schedule().card(&user, &message_id) else {
            return;
        };
        let Some(contribution) = card.contribution.as_ref() else {
            return;
        };
        let stored = match self.messages.get(&message_id).await {
            Ok(Some(stored)) => stored,
            Ok(None) => {
                self.schedule().forget(&message_id);
                return;
            }
            Err(error) => {
                tracing::debug!(target: "flare_sdk.plugin", %error, "viewer state: read message failed");
                return;
            }
        };
        let (local, shared_version) = layers_of(&card.message, contribution, &stored);
        let decision = self.schedule().decide(
            &user,
            &message_id,
            trigger,
            local.as_ref(),
            shared_version,
            now_millis(),
        );
        self.act(user, message_id, decision);
    }

    fn act(self: &Arc<Self>, user: String, message_id: String, decision: ViewerDecision) {
        match decision {
            ViewerDecision::Skip => {}
            ViewerDecision::Read => {
                let this = self.clone();
                crate::shared::util::background::spawn_background(async move {
                    this.read(user, message_id).await;
                });
            }
            ViewerDecision::RetryAt(at_ms) => {
                let this = self.clone();
                crate::shared::util::background::spawn_background(async move {
                    let wait = at_ms.saturating_sub(now_millis());
                    crate::shared::util::delay(std::time::Duration::from_millis(wait)).await;
                    // A host without timers (a browser worker has no window) returns at once:
                    // leave the read to the next presentation or event instead of spinning.
                    if now_millis().saturating_add(50) < at_ms {
                        return;
                    }
                    this.judge(message_id, ViewerTrigger::Retry).await;
                });
            }
        }
    }

    async fn read(self: Arc<Self>, user: String, message_id: String) {
        let ok = {
            let _permit = self.permits.acquire().await;
            self.read_once(&user, &message_id).await
        };
        let again = self
            .schedule()
            .finished(&user, &message_id, ok, now_millis());
        if again {
            let this = self.clone();
            crate::shared::util::background::spawn_background(async move {
                this.judge(message_id, ViewerTrigger::Retry).await;
            });
        }
    }

    /// One read. `false` only for a failure worth retrying after the backoff.
    async fn read_once(&self, user: &str, message_id: &str) -> bool {
        let Some(card) = self.schedule().card(user, message_id) else {
            return true;
        };
        let (Some(contribution), Some(viewer)) =
            (card.contribution.as_ref(), card.viewer().cloned())
        else {
            return true;
        };
        let Ok(Some(stored)) = self.messages.get(message_id).await else {
            return true;
        };
        let entity = contribution.entity_of(&card.message.payload);
        let state = CardState::of(Some(&stored), entity.as_deref());
        if state.unbound {
            return true;
        }
        let layers = state.layers();
        let seen_shared = layers
            .applied_shared(contribution, &card.message.payload)
            .map_or(0, |shared| shared.version);
        let input =
            match viewer.bind_input(&layers.merge_shared(contribution, &card.message.payload)) {
                Ok(input) => input,
                Err(_) => {
                    tracing::warn!(
                        target: "flare_sdk.plugin",
                        plugin_id = %card.message.plugin_id,
                        "viewer state: the card data does not bind the declared input"
                    );
                    return true;
                }
            };
        let data = match self
            .api
            .dispatch_card(
                &viewer.operation,
                input,
                Some(&card.conversation_id),
                Some(message_id),
            )
            .await
            .and_then(super::CapabilityDispatchResult::fail_if_unsuccessful)
        {
            Ok(response) => response.data,
            Err(error) => {
                tracing::debug!(
                    target: "flare_sdk.plugin",
                    plugin_id = %card.message.plugin_id,
                    %error,
                    "viewer state read failed"
                );
                if plugin_errors::invalidates_catalog(&error) {
                    self.api.invalidate_catalog_reads();
                    self.announce_recent(Some(&card.message.plugin_id)).await;
                }
                return !plugin_errors::is_retryable(&error);
            }
        };
        if self.user().await != user {
            return true;
        }
        self.schedule()
            .narrow(user, message_id, UiViewerState::result_refresh_on(&data));
        // The layer may have moved while the call was out (an action result): build on what is stored now.
        let previous = match self.messages.get(message_id).await {
            Ok(Some(now)) => now.plugin_message_state(),
            _ => return true,
        };
        let Some(layer) = UiViewerState::local_layer(&data, previous.as_ref(), seen_shared) else {
            tracing::warn!(
                target: "flare_sdk.plugin",
                plugin_id = %card.message.plugin_id,
                "viewer state: the plugin returned a result that is not a valid viewer layer"
            );
            return true;
        };
        match self
            .messages
            .apply_plugin_message_state(message_id, &layer)
            .await
        {
            Ok(OperationApplyResult::Applied) => {
                self.bus
                    .publish(SdkEvent::Message(MessageEvent::PluginStateChanged {
                        conversation_id: card.conversation_id,
                        message_id: message_id.to_owned(),
                        version: layer.version,
                    }));
            }
            Ok(result) => tracing::debug!(
                target: "flare_sdk.plugin",
                ?result,
                "viewer state not applied (a newer layer was written meanwhile)"
            ),
            Err(error) => tracing::warn!(
                target: "flare_sdk.plugin",
                %error,
                "viewer state: write failed"
            ),
        }
        true
    }

    /// Announce every card presented recently (of `plugin_id`, when given) with
    /// `PluginStateChanged`, so hosts present them again: after the catalog was
    /// invalidated, a disabled plugin's cards turn inert and a re-enabled one's
    /// come back. `version` is the card's stored local version (the shared one
    /// when it has no local layer).
    pub(crate) async fn announce_recent(&self, plugin_id: Option<&str>) {
        let user = self.user().await;
        let cards = self.schedule().recent(&user, plugin_id, now_millis());
        for card in cards {
            let version = match self.messages.get(&card.message_id).await {
                Ok(Some(stored)) => stored
                    .plugin_message_state()
                    .map_or_else(|| stored.plugin_shared_state_version(), |l| l.version),
                _ => continue,
            };
            self.bus
                .publish(SdkEvent::Message(MessageEvent::PluginStateChanged {
                    conversation_id: card.conversation_id,
                    message_id: card.message_id,
                    version,
                }));
        }
    }
}
