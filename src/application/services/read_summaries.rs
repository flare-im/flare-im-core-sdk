//! 群聊已读摘要：给视图里本人发出的群消息补上 `receipt.readSummary`（见 `crate::model::receipt`）。
//!
//! 数据来自 Core 的已读摘要查询（[`ReadReceiptTransport`]）。规则：
//! - 只为视图里正在展示的消息取：时间线、搜索等视图消息经 `MessageViewAssembler` 水合时调用
//!   [`ReadSummaryService::decorate`]，命中缓存的直接带上，缺的或过期的记入该会话的待取集合；
//! - 按会话合并成批（每批至多 [`MAX_SUMMARY_BATCH`] 条，一轮至多 [`MAX_IDS_PER_PASS`] 条，新的优先），
//!   同一会话两次请求至少间隔 [`MIN_FETCH_INTERVAL_MS`]，在途时再来的需求等这一轮结束后合并再取；
//! - 超大群与非群聊：Core 回 `available: false`（成员数超过 [`LARGE_GROUP_MEMBERS`]），该会话
//!   [`UNAVAILABLE_TTL_MS`] 内不再请求，消息不带摘要；
//! - 增量刷新：群里的已读回执（`MessageEvent::ReadReceipt`）到达后，缓存里该会话 `seq <= read_seq`
//!   且仍有未读的本人消息标记过期，去抖 [`READ_EVENT_DEBOUNCE_MS`] 后重取；
//! - 取到的摘要有变化时发 `MessageEvent::ReadSummariesChanged`，打开的视图据此重读，增量带出新摘要；
//! - 失败：暂时性错误（网络、超时、限流、5xx）按间隔翻倍退避，至多 [`MAX_BACKOFF_MS`]；其余（无权限、
//!   不是成员、参数非法）该会话按不可用处理；
//! - 内存有界：至多 [`MAX_CONVERSATIONS`] 个会话、每会话 [`MAX_ENTRIES`] 条，按最近使用淘汰；换登录用户清空。
//!
//! 已读 / 未读成员名单（ReadReceiptSheet）走 [`ReadSummaryService::members`]，直接透传 Core 的分页，
//! 顺带用返回的摘要刷新缓存。
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::kernel::CurrentUserIdStore;
use crate::kernel::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::message::{ConversationType, MessageStatus};
use crate::model::{IMMessage, MessageReadSummary};
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::{delay, now_millis, spawn_background};

/// 一次摘要请求的消息数上限（与 Core 一致）。
pub(crate) const MAX_SUMMARY_BATCH: usize = 50;
/// 一轮至多取的消息数（两批）。
pub(crate) const MAX_IDS_PER_PASS: usize = 100;
/// Core 不给摘要的群规模：在席成员超过它时 `available: false`。
pub(crate) const LARGE_GROUP_MEMBERS: u32 = 500;
/// 同一会话两次请求的最小间隔。
pub(crate) const MIN_FETCH_INTERVAL_MS: u64 = 2_000;
/// 已读回执到达后重取前的去抖。
pub(crate) const READ_EVENT_DEBOUNCE_MS: u64 = 800;
/// 缓存的摘要多久后视为过期（视图再次水合时重取）。
pub(crate) const SUMMARY_TTL_MS: u64 = 5 * 60_000;
/// 会话不可用（超大群、非群聊、无权）后多久内不再请求。
pub(crate) const UNAVAILABLE_TTL_MS: u64 = 10 * 60_000;
/// 暂时性失败的退避上限。
pub(crate) const MAX_BACKOFF_MS: u64 = 60_000;
pub(crate) const MAX_CONVERSATIONS: usize = 16;
pub(crate) const MAX_ENTRIES: usize = 1_000;
/// 待取集合上限，超出时丢最旧的（视图下次水合会再提）。
const MAX_WANTED: usize = 200;
/// 名单分页大小（与 Core 一致：默认 50，最大 200）。
const DEFAULT_MEMBERS_LIMIT: u32 = 50;
const MAX_MEMBERS_LIMIT: u32 = 200;

/// Core 的一页摘要。`summaries` 只含本会话、调用者本人发出、未撤回或删除的消息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SummaryPage {
    pub available: bool,
    pub summaries: Vec<(String, MessageReadSummary)>,
}

/// 名单里的哪一侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageReaderState {
    Read,
    Unread,
}
impl MessageReaderState {
    pub(crate) fn wire(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Unread => "unread",
        }
    }
}

/// 名单查询（绑定入参）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadReceiptMembersRequest {
    pub conversation_id: String,
    pub server_msg_id: String,
    pub state: MessageReaderState,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// 名单的一页（绑定出参）：按 user_id 升序，`nextCursor` 在 `hasMore` 时给出。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadReceiptMembersPage {
    pub conversation_id: String,
    pub server_msg_id: String,
    /// false：非群聊或超大群，名单不可用（`userIds` 为空）。
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<MessageReadSummary>,
    pub user_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// 访问 Core 已读摘要与名单的传输。
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub(crate) trait ReadReceiptTransport: Send + Sync {
    async fn summaries(
        &self,
        conversation_id: &str,
        server_msg_ids: &[String],
    ) -> Result<SummaryPage>;
    async fn members(&self, request: &ReadReceiptMembersRequest) -> Result<ReadReceiptMembersPage>;
}

#[derive(Debug, Clone, Copy)]
struct Cached {
    seq: u64,
    /// `None`：Core 没有给它（不是本人的、已撤回删除的），不再取。
    summary: Option<MessageReadSummary>,
    fetched_at_ms: u64,
    stale: bool,
    used_at_ms: u64,
}

#[derive(Default)]
struct ConversationState {
    entries: HashMap<String, Cached>,
    /// 待取：server id → seq（新的优先取）。
    wanted: BTreeMap<String, u64>,
    unavailable_until_ms: u64,
    in_flight: bool,
    /// 下一次最早可请求的时刻（间隔与退避）。
    not_before_ms: u64,
    /// 已排了一次延迟的请求。
    armed: bool,
    failures: u32,
    used_at_ms: u64,
}

impl ConversationState {
    fn want(&mut self, server_id: &str, seq: u64) {
        self.wanted.insert(server_id.to_owned(), seq);
        while self.wanted.len() > MAX_WANTED {
            let oldest = self
                .wanted
                .iter()
                .min_by_key(|(_, seq)| **seq)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => self.wanted.remove(&id),
                None => break,
            };
        }
    }
    /// 本轮要取的：新的（seq 大的）优先，至多 [`MAX_IDS_PER_PASS`] 条。
    fn take_pass(&mut self) -> Vec<(String, u64)> {
        let mut wanted: Vec<(String, u64)> = std::mem::take(&mut self.wanted).into_iter().collect();
        wanted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let rest = wanted.split_off(wanted.len().min(MAX_IDS_PER_PASS));
        self.wanted.extend(rest);
        wanted
    }
    fn trim(&mut self) {
        if self.entries.len() <= MAX_ENTRIES {
            return;
        }
        let mut by_use: Vec<(u64, u64, String)> = self
            .entries
            .iter()
            .map(|(id, e)| (e.used_at_ms, e.seq, id.clone()))
            .collect();
        by_use.sort();
        for (_, _, id) in by_use.into_iter().take(self.entries.len() - MAX_ENTRIES) {
            self.entries.remove(&id);
        }
    }
}

#[derive(Default)]
struct State {
    user: String,
    conversations: HashMap<String, ConversationState>,
}

impl State {
    fn for_user(&mut self, user: &str) -> &mut Self {
        if self.user != user {
            self.user = user.to_owned();
            self.conversations.clear();
        }
        self
    }
    fn conversation(&mut self, conversation_id: &str, now: u64) -> &mut ConversationState {
        if !self.conversations.contains_key(conversation_id)
            && self.conversations.len() >= MAX_CONVERSATIONS
            && let Some(oldest) = self
                .conversations
                .iter()
                .filter(|(_, c)| !c.in_flight)
                .min_by_key(|(_, c)| c.used_at_ms)
                .map(|(id, _)| id.clone())
        {
            self.conversations.remove(&oldest);
        }
        let conversation = self
            .conversations
            .entry(conversation_id.to_owned())
            .or_default();
        conversation.used_at_ms = now;
        conversation
    }
}

pub(crate) struct ReadSummaryService {
    transport: Arc<dyn ReadReceiptTransport>,
    bus: EventBus,
    current_user: CurrentUserIdStore,
    state: Mutex<State>,
    /// 自身的弱引用，后台任务用它回到服务（服务释放后任务自然结束）。
    me: Weak<ReadSummaryService>,
}

/// 视图里需要摘要的消息：群聊、本人发出、已有服务端 id 与序号、未撤回未删除。
fn wants_summary(message: &IMMessage, user: &str) -> bool {
    message.conversation_type == ConversationType::Group as i32
        && !user.is_empty()
        && message.sender_id == user
        && !message.server_id.trim().is_empty()
        && message.conversation_seq > 0
        && !message.is_recalled
        && message.status != MessageStatus::Recalled as i32
        && message.status != MessageStatus::Deleted as i32
}

fn http_status(error: &FlareError) -> Option<u16> {
    match error {
        FlareError::Localized {
            params: Some(params),
            ..
        } => params.get("httpStatus").and_then(|s| s.parse().ok()),
        _ => None,
    }
}
/// 暂时性失败：传输失败（无 HTTP 状态）、限流、5xx。
fn is_transient(error: &FlareError) -> bool {
    match http_status(error) {
        Some(status) => status == 429 || status >= 500,
        None => !matches!(
            error.code(),
            Some(
                ErrorCode::InvalidParameter
                    | ErrorCode::PermissionDenied
                    | ErrorCode::NotConnected
                    | ErrorCode::MessageNotFound
            )
        ),
    }
}

impl ReadSummaryService {
    pub(crate) fn new(
        transport: Arc<dyn ReadReceiptTransport>,
        bus: EventBus,
        current_user: CurrentUserIdStore,
    ) -> Arc<Self> {
        let service = Arc::new_cyclic(|me| Self {
            transport,
            bus: bus.clone(),
            current_user,
            state: Mutex::new(State::default()),
            me: me.clone(),
        });
        Self::listen_read_receipts(Arc::downgrade(&service), bus);
        service
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 给视图消息带上缓存的摘要；缺的、过期的记入待取并排一次请求。
    pub(crate) async fn decorate(&self, messages: &mut [IMMessage]) {
        let user = self.current_user.read().await.trim().to_owned();
        if user.is_empty() {
            return;
        }
        let now = now_millis();
        let mut schedule = Vec::new();
        {
            let mut state = self.state();
            let state = state.for_user(&user);
            for message in messages.iter_mut() {
                if !wants_summary(message, &user) {
                    continue;
                }
                let conversation = state.conversation(&message.conversation_id, now);
                if conversation.unavailable_until_ms > now {
                    message.set_read_summary(None);
                    continue;
                }
                let server_id = message.server_id.trim().to_owned();
                let cached = conversation.entries.get_mut(&server_id);
                let fresh = match cached {
                    Some(cached) => {
                        cached.used_at_ms = now;
                        message.set_read_summary(cached.summary);
                        !cached.stale && now.saturating_sub(cached.fetched_at_ms) < SUMMARY_TTL_MS
                    }
                    None => {
                        message.set_read_summary(None);
                        false
                    }
                };
                if !fresh {
                    conversation.want(&server_id, message.conversation_seq);
                    if !schedule.contains(&message.conversation_id) {
                        schedule.push(message.conversation_id.clone());
                    }
                }
            }
        }
        for conversation_id in schedule {
            self.schedule(&conversation_id, &user, 0);
        }
    }

    /// 排一次该会话的请求：在途时等它结束；太早时延迟到可请求的时刻；`after_ms` 是额外的去抖。
    fn schedule(&self, conversation_id: &str, user: &str, after_ms: u64) {
        let now = now_millis();
        let wait = {
            let mut state = self.state();
            if state.user != user {
                return;
            }
            let Some(conversation) = state.conversations.get_mut(conversation_id) else {
                return;
            };
            if conversation.in_flight || conversation.armed || conversation.wanted.is_empty() {
                return;
            }
            conversation.armed = true;
            conversation.not_before_ms.saturating_sub(now).max(after_ms)
        };
        let Some(me) = self.me.upgrade() else {
            return;
        };
        let conversation_id = conversation_id.to_owned();
        let user = user.to_owned();
        spawn_background(async move {
            if wait > 0 {
                delay(Duration::from_millis(wait)).await;
            }
            me.run_pass(&conversation_id, &user).await;
        });
    }

    async fn run_pass(&self, conversation_id: &str, user: &str) {
        let pass = {
            let mut state = self.state();
            if state.user != user {
                return;
            }
            let Some(conversation) = state.conversations.get_mut(conversation_id) else {
                return;
            };
            conversation.armed = false;
            if conversation.in_flight {
                return;
            }
            let pass = conversation.take_pass();
            if pass.is_empty() {
                return;
            }
            conversation.in_flight = true;
            pass
        };
        let mut changed: Vec<String> = Vec::new();
        let mut outcome: std::result::Result<(), FlareError> = Ok(());
        let mut unavailable = false;
        for batch in pass.chunks(MAX_SUMMARY_BATCH) {
            let ids: Vec<String> = batch.iter().map(|(id, _)| id.clone()).collect();
            match self.transport.summaries(conversation_id, &ids).await {
                Ok(page) if !page.available => {
                    unavailable = true;
                    break;
                }
                Ok(page) => {
                    let found: HashMap<String, MessageReadSummary> =
                        page.summaries.into_iter().collect();
                    changed.extend(self.store(conversation_id, user, batch, &found));
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        let now = now_millis();
        {
            let mut state = self.state();
            if state.user != user {
                return;
            }
            let Some(conversation) = state.conversations.get_mut(conversation_id) else {
                return;
            };
            conversation.in_flight = false;
            match outcome {
                _ if unavailable => {
                    conversation.unavailable_until_ms = now + UNAVAILABLE_TTL_MS;
                    conversation.wanted.clear();
                    changed.extend(
                        conversation
                            .entries
                            .iter()
                            .filter(|(_, e)| e.summary.is_some())
                            .map(|(id, _)| id.clone()),
                    );
                    conversation.entries.clear();
                }
                Ok(()) => {
                    conversation.failures = 0;
                    conversation.not_before_ms = now + MIN_FETCH_INTERVAL_MS;
                }
                Err(error) if is_transient(&error) => {
                    conversation.failures = conversation.failures.saturating_add(1);
                    let backoff = MIN_FETCH_INTERVAL_MS
                        .saturating_mul(1 << conversation.failures.min(8))
                        .min(MAX_BACKOFF_MS);
                    conversation.not_before_ms = now + backoff;
                    // What this pass took goes back to be tried again.
                    for (id, seq) in &pass {
                        conversation.want(id, *seq);
                    }
                    tracing::debug!(target: "flare_sdk.read_receipt", %error, conversation_id, "read summaries failed; backing off");
                }
                Err(error) => {
                    tracing::debug!(target: "flare_sdk.read_receipt", %error, conversation_id, "read summaries refused; conversation skipped");
                    conversation.unavailable_until_ms = now + UNAVAILABLE_TTL_MS;
                    conversation.wanted.clear();
                }
            }
        }
        if !changed.is_empty() {
            changed.sort();
            changed.dedup();
            self.bus
                .publish(SdkEvent::Message(MessageEvent::ReadSummariesChanged {
                    conversation_id: conversation_id.to_owned(),
                    message_ids: changed,
                }));
        }
        self.schedule(conversation_id, user, 0);
    }

    /// 写入一批结果；返回摘要变了的消息。
    fn store(
        &self,
        conversation_id: &str,
        user: &str,
        batch: &[(String, u64)],
        found: &HashMap<String, MessageReadSummary>,
    ) -> Vec<String> {
        let now = now_millis();
        let mut state = self.state();
        if state.user != user {
            return Vec::new();
        }
        let conversation = state.conversation(conversation_id, now);
        let mut changed = Vec::new();
        for (id, seq) in batch {
            let summary = found
                .get(id)
                .copied()
                .filter(MessageReadSummary::is_displayable);
            let previous = conversation.entries.insert(
                id.clone(),
                Cached {
                    seq: *seq,
                    summary,
                    fetched_at_ms: now,
                    stale: false,
                    used_at_ms: now,
                },
            );
            if previous.and_then(|p| p.summary) != summary {
                changed.push(id.clone());
            }
        }
        conversation.trim();
        changed
    }

    /// 已读回执到达：该会话缓存里 `seq <= read_seq`、仍有未读的本人消息过期并去抖重取。
    pub(crate) fn on_read_receipt(&self, conversation_id: &str, reader: &str, read_seq: u64) {
        let user = {
            let mut state = self.state();
            if state.user.is_empty() || reader == state.user || read_seq == 0 {
                return;
            }
            let user = state.user.clone();
            let Some(conversation) = state.conversations.get_mut(conversation_id) else {
                return;
            };
            if conversation.unavailable_until_ms > now_millis() {
                return;
            }
            let stale: Vec<(String, u64)> = conversation
                .entries
                .iter_mut()
                .filter(|(_, e)| e.seq <= read_seq && e.summary.is_some_and(|s| s.unread_count > 0))
                .map(|(id, e)| {
                    e.stale = true;
                    (id.clone(), e.seq)
                })
                .collect();
            if stale.is_empty() {
                return;
            }
            for (id, seq) in stale {
                conversation.want(&id, seq);
            }
            user
        };
        self.schedule(conversation_id, &user, READ_EVENT_DEBOUNCE_MS);
    }

    fn listen_read_receipts(service: Weak<Self>, bus: EventBus) {
        let mut events = bus.subscribe();
        spawn_background(async move {
            while let Ok(event) = events.recv().await {
                let SdkEvent::Message(MessageEvent::ReadReceipt {
                    conversation_id,
                    event,
                }) = event
                else {
                    continue;
                };
                let Some(service) = service.upgrade() else {
                    break;
                };
                service.on_read_receipt(&conversation_id, &event.user_id, event.read_seq);
            }
        });
    }

    /// 已读 / 未读成员的一页（ReadReceiptSheet）。返回的摘要顺带刷新缓存。
    pub(crate) async fn members(
        &self,
        mut request: ReadReceiptMembersRequest,
    ) -> Result<ReadReceiptMembersPage> {
        let user = self.current_user.read().await.trim().to_owned();
        if user.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::NotConnected,
                "sdk.read_receipt.session_required",
            ));
        }
        let valid_id = |id: &str| {
            !id.trim().is_empty()
                && id.len() <= 256
                && !id.chars().any(|c| c.is_control() || c.is_whitespace())
        };
        if !valid_id(&request.conversation_id)
            || !valid_id(&request.server_msg_id)
            || request
                .cursor
                .as_deref()
                .is_some_and(|c| c.len() > 512 || c.chars().any(char::is_control))
        {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "sdk.read_receipt.invalid_request",
            ));
        }
        request.limit = Some(
            request
                .limit
                .unwrap_or(DEFAULT_MEMBERS_LIMIT)
                .clamp(1, MAX_MEMBERS_LIMIT),
        );
        let page = self.transport.members(&request).await?;
        if let Some(summary) = page.summary.filter(|_| page.available) {
            let seq = {
                let state = self.state();
                state
                    .conversations
                    .get(&request.conversation_id)
                    .and_then(|c| c.entries.get(&request.server_msg_id))
                    .map(|e| e.seq)
            };
            if let Some(seq) = seq {
                let found = HashMap::from([(request.server_msg_id.clone(), summary)]);
                let changed = self.store(
                    &request.conversation_id,
                    &user,
                    &[(request.server_msg_id.clone(), seq)],
                    &found,
                );
                if !changed.is_empty() {
                    self.bus
                        .publish(SdkEvent::Message(MessageEvent::ReadSummariesChanged {
                            conversation_id: request.conversation_id.clone(),
                            message_ids: changed,
                        }));
                }
            }
        }
        Ok(page)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::RwLock;

    #[derive(Default)]
    struct FakeTransport {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        available: std::sync::atomic::AtomicBool,
        unread: AtomicUsize,
        fail_with: Mutex<Option<FlareError>>,
    }
    #[async_trait]
    impl ReadReceiptTransport for FakeTransport {
        async fn summaries(
            &self,
            conversation_id: &str,
            server_msg_ids: &[String],
        ) -> Result<SummaryPage> {
            self.calls
                .lock()
                .unwrap()
                .push((conversation_id.to_owned(), server_msg_ids.to_vec()));
            if let Some(error) = self.fail_with.lock().unwrap().take() {
                return Err(error);
            }
            if !self.available.load(Ordering::SeqCst) {
                return Ok(SummaryPage::default());
            }
            let unread = self.unread.load(Ordering::SeqCst) as u32;
            Ok(SummaryPage {
                available: true,
                summaries: server_msg_ids
                    .iter()
                    .filter(|id| !id.starts_with("other"))
                    .map(|id| {
                        (
                            id.clone(),
                            MessageReadSummary {
                                read_count: 4 - unread,
                                unread_count: unread,
                                total: 4,
                            },
                        )
                    })
                    .collect(),
            })
        }
        async fn members(
            &self,
            request: &ReadReceiptMembersRequest,
        ) -> Result<ReadReceiptMembersPage> {
            Ok(ReadReceiptMembersPage {
                conversation_id: request.conversation_id.clone(),
                server_msg_id: request.server_msg_id.clone(),
                available: true,
                summary: Some(MessageReadSummary {
                    read_count: 4,
                    unread_count: 0,
                    total: 4,
                }),
                user_ids: vec!["u2".into()],
                next_cursor: None,
                has_more: false,
            })
        }
    }

    fn group_message(server_id: &str, sender: &str, seq: u64) -> IMMessage {
        let mut message = IMMessage::new(flare_proto::common::Message {
            server_id: server_id.into(),
            client_msg_id: format!("c-{server_id}"),
            conversation_id: "group-1".into(),
            conversation_type: ConversationType::Group as i32,
            sender_id: sender.into(),
            conversation_seq: seq,
            status: MessageStatus::Persisted as i32,
            ..Default::default()
        });
        message.conversation_seq = seq;
        message
    }

    fn service(transport: Arc<FakeTransport>) -> (Arc<ReadSummaryService>, EventBus) {
        let bus = EventBus::new();
        let user: CurrentUserIdStore = Arc::new(RwLock::new("me".into()));
        (ReadSummaryService::new(transport, bus.clone(), user), bus)
    }

    async fn settle() {
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    #[tokio::test]
    async fn own_group_messages_are_fetched_in_batches_and_decorated_from_the_cache() {
        let transport = Arc::new(FakeTransport::default());
        transport.available.store(true, Ordering::SeqCst);
        transport.unread.store(1, Ordering::SeqCst);
        let (service, bus) = service(transport.clone());
        let mut changes = bus.subscribe();
        let mut messages: Vec<IMMessage> = (1..=60)
            .map(|seq| group_message(&format!("s{seq}"), "me", seq))
            .collect();
        messages.push(group_message("peer", "u2", 61));
        let mut single = group_message("single", "me", 62);
        single.conversation_type = ConversationType::Single as i32;
        messages.push(single);
        service.decorate(&mut messages).await;
        assert!(
            messages.iter().all(|m| m.read_summary().is_none()),
            "nothing cached yet"
        );
        settle().await;
        let calls = transport.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2, "60 own messages in two batches");
        assert_eq!(calls[0].1.len(), 50);
        assert_eq!(calls[0].1[0], "s60", "newest first");
        assert!(
            !calls
                .iter()
                .flat_map(|c| &c.1)
                .any(|id| id == "peer" || id == "single"),
            "only own group messages"
        );
        let mut announced = Vec::new();
        while let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_millis(100), changes.recv()).await
        {
            if let SdkEvent::Message(MessageEvent::ReadSummariesChanged {
                conversation_id,
                message_ids,
            }) = event
            {
                assert_eq!(conversation_id, "group-1");
                announced.extend(message_ids);
            }
        }
        assert!(
            announced.iter().any(|id| id == "s60"),
            "views are told to read again, naming the messages: {announced:?}"
        );
        assert!(
            !announced.iter().any(|id| id == "peer" || id == "single"),
            "only messages whose summary moved are named"
        );
        service.decorate(&mut messages).await;
        assert_eq!(
            messages[0].receipt_view().read_summary,
            Some(MessageReadSummary {
                read_count: 3,
                unread_count: 1,
                total: 4
            })
        );
        assert!(messages[60].read_summary().is_none());
        settle().await;
        assert_eq!(
            transport.calls.lock().unwrap().len(),
            2,
            "fresh cache, no refetch"
        );
    }

    #[tokio::test]
    async fn large_groups_are_not_asked_again() {
        let transport = Arc::new(FakeTransport::default());
        let (service, _bus) = service(transport.clone());
        let mut messages = vec![group_message("s1", "me", 1)];
        service.decorate(&mut messages).await;
        settle().await;
        service.decorate(&mut messages).await;
        settle().await;
        assert_eq!(transport.calls.lock().unwrap().len(), 1);
        assert!(messages[0].read_summary().is_none());
    }

    #[tokio::test]
    async fn read_receipts_refresh_only_older_unread_own_messages_after_a_debounce() {
        let transport = Arc::new(FakeTransport::default());
        transport.available.store(true, Ordering::SeqCst);
        transport.unread.store(2, Ordering::SeqCst);
        let (service, bus) = service(transport.clone());
        let mut messages = vec![group_message("s1", "me", 1), group_message("s5", "me", 5)];
        service.decorate(&mut messages).await;
        settle().await;
        assert_eq!(transport.calls.lock().unwrap().len(), 1);
        transport.unread.store(0, Ordering::SeqCst);
        // My own read receipt changes nothing.
        service.on_read_receipt("group-1", "me", 9);
        // Another member read up to seq 3, through the bus.
        bus.publish(SdkEvent::Message(MessageEvent::ReadReceipt {
            conversation_id: "group-1".into(),
            event: flare_proto::common::ReadReceiptEvent {
                conversation_id: "group-1".into(),
                user_id: "u2".into(),
                read_seq: 3,
                ..Default::default()
            },
        }));
        settle().await;
        assert_eq!(transport.calls.lock().unwrap().len(), 1, "debounced");
        tokio::time::sleep(Duration::from_millis(
            READ_EVENT_DEBOUNCE_MS.max(MIN_FETCH_INTERVAL_MS) + 200,
        ))
        .await;
        let calls = transport.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].1, vec!["s1".to_string()], "only seq <= read_seq");
        service.decorate(&mut messages).await;
        assert_eq!(
            messages[0].receipt_view().status,
            crate::model::MessageReceiptStatus::Read
        );
        assert_eq!(messages[1].read_summary().unwrap().unread_count, 2);
    }

    #[tokio::test]
    async fn transient_failures_back_off_and_refusals_skip_the_conversation() {
        let transport = Arc::new(FakeTransport::default());
        transport.available.store(true, Ordering::SeqCst);
        *transport.fail_with.lock().unwrap() = Some(FlareError::localized(
            ErrorCode::ServiceUnavailable,
            "unavailable",
        ));
        let (service, _bus) = service(transport.clone());
        let mut messages = vec![group_message("s1", "me", 1)];
        service.decorate(&mut messages).await;
        settle().await;
        assert_eq!(transport.calls.lock().unwrap().len(), 1);
        {
            let state = service.state();
            let conversation = &state.conversations["group-1"];
            assert_eq!(conversation.failures, 1);
            assert!(conversation.wanted.contains_key("s1"), "kept for the retry");
            assert!(conversation.armed, "a retry is scheduled");
        }
        let mut params = HashMap::new();
        params.insert("httpStatus".to_string(), "403".to_string());
        let refused = FlareError::Localized {
            code: ErrorCode::PermissionDenied,
            reason: "denied".into(),
            details: None,
            params: Some(params),
            timestamp: chrono::Utc::now(),
        };
        assert!(!is_transient(&refused));
        assert!(is_transient(&FlareError::localized(
            ErrorCode::ServiceUnavailable,
            "x"
        )));
    }

    #[tokio::test]
    async fn the_members_page_passes_through_and_refreshes_a_cached_summary() {
        let transport = Arc::new(FakeTransport::default());
        transport.available.store(true, Ordering::SeqCst);
        transport.unread.store(1, Ordering::SeqCst);
        let (service, _bus) = service(transport.clone());
        let mut messages = vec![group_message("s1", "me", 1)];
        service.decorate(&mut messages).await;
        settle().await;
        let page = service
            .members(ReadReceiptMembersRequest {
                conversation_id: "group-1".into(),
                server_msg_id: "s1".into(),
                state: MessageReaderState::Read,
                cursor: None,
                limit: Some(1000),
            })
            .await
            .unwrap();
        assert_eq!(page.user_ids, vec!["u2".to_string()]);
        service.decorate(&mut messages).await;
        assert_eq!(messages[0].read_summary().unwrap().unread_count, 0);
        assert!(
            service
                .members(ReadReceiptMembersRequest {
                    conversation_id: "group 1".into(),
                    server_msg_id: "s1".into(),
                    state: MessageReaderState::Unread,
                    cursor: None,
                    limit: None,
                })
                .await
                .is_err()
        );
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["userIds"], serde_json::json!(["u2"]));
        assert_eq!(json["hasMore"], serde_json::json!(false));
    }
}
