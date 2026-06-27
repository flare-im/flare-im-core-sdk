use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::{RwLock, mpsc::error::TryRecvError};

use crate::client::api::ConversationApi;
use crate::kernel::event::{ConversationEvent, EventBus, MessageEvent, SdkEvent, SyncNotify};
use crate::model::{
    BootstrapHomeTimelineRequest, CloseViewRequest, CloseViewResponse, Conversation,
    ConversationTimelineSnapshot, HomeTimelineSnapshot, IMMessage, LoadOlderTimelineViewRequest,
    OpenConversationListViewRequest, OpenConversationTimelineRequest, OpenTimelineViewRequest,
    ViewDelta, ViewDeltaKind, ViewDeltaOp, ViewLoadOlderResponse, ViewOpenResponse, ViewSnapshot,
    ViewUpdate, ViewUpdateKind, normalized_conversation_limit, normalized_message_limit,
};
use crate::shared::error::{ErrorCode, FlareError, Result};
use crate::shared::util::{BackgroundTask, delay, spawn_background_task};

const VIEW_REFRESH_DEBOUNCE: Duration = Duration::from_millis(40);
const MAX_TIMELINE_VIEWS: usize = 8;
const MAX_CONVERSATION_LIST_VIEWS: usize = 2;
const MAX_TIMELINE_WINDOW_MESSAGES: usize = 500;

#[derive(Clone)]
pub struct ViewApi {
    inner: Arc<ViewApiInner>,
}

struct ViewApiInner {
    conversation_api: ConversationApi,
    bus: EventBus,
    views: RwLock<ViewRegistrations>,
    next_id: AtomicU64,
    refresh_worker: Mutex<Option<BackgroundTask>>,
}

impl Drop for ViewApiInner {
    fn drop(&mut self) {
        if let Ok(worker) = self.refresh_worker.get_mut()
            && let Some(worker) = worker.take()
        {
            worker.abort();
        }
    }
}

#[derive(Default)]
struct ViewRegistrations {
    timelines: HashMap<String, TimelineViewRegistration>,
    conversation_lists: HashMap<String, ConversationListViewRegistration>,
}

struct TimelineViewRegistration {
    request: OpenTimelineViewRequest,
    order: u64,
    last_snapshot: ConversationTimelineSnapshot,
}

struct ConversationListViewRegistration {
    request: OpenConversationListViewRequest,
    order: u64,
    last_snapshot: HomeTimelineSnapshot,
}

impl ViewRegistrations {
    fn insert_timeline(
        &mut self,
        view_id: String,
        request: OpenTimelineViewRequest,
        order: u64,
        last_snapshot: ConversationTimelineSnapshot,
    ) {
        let conversation_id = request.conversation_id.clone();
        self.timelines
            .retain(|_, view| view.request.conversation_id != conversation_id);
        self.timelines.insert(
            view_id,
            TimelineViewRegistration {
                request,
                order,
                last_snapshot,
            },
        );
        evict_oldest(&mut self.timelines, MAX_TIMELINE_VIEWS);
    }

    fn insert_conversation_list(
        &mut self,
        view_id: String,
        request: OpenConversationListViewRequest,
        order: u64,
        last_snapshot: HomeTimelineSnapshot,
    ) {
        self.conversation_lists.insert(
            view_id,
            ConversationListViewRegistration {
                request,
                order,
                last_snapshot,
            },
        );
        evict_oldest(&mut self.conversation_lists, MAX_CONVERSATION_LIST_VIEWS);
    }
}

trait OrderedViewRegistration {
    fn order(&self) -> u64;
}

impl OrderedViewRegistration for TimelineViewRegistration {
    fn order(&self) -> u64 {
        self.order
    }
}

impl OrderedViewRegistration for ConversationListViewRegistration {
    fn order(&self) -> u64 {
        self.order
    }
}

fn evict_oldest<T: OrderedViewRegistration>(views: &mut HashMap<String, T>, limit: usize) {
    while views.len() > limit {
        let Some(oldest_id) = views
            .iter()
            .min_by_key(|(_, view)| view.order())
            .map(|(view_id, _)| view_id.clone())
        else {
            return;
        };
        views.remove(&oldest_id);
    }
}

impl ViewApi {
    pub fn new(conversation_api: ConversationApi, bus: EventBus) -> Self {
        let api = Self {
            inner: Arc::new(ViewApiInner {
                conversation_api,
                bus,
                views: RwLock::new(ViewRegistrations::default()),
                next_id: AtomicU64::new(1),
                refresh_worker: Mutex::new(None),
            }),
        };
        let worker = api.spawn_refresh_worker();
        if let Ok(mut slot) = api.inner.refresh_worker.lock() {
            *slot = Some(worker);
        }
        api
    }

    pub async fn open_timeline(
        &self,
        request: OpenTimelineViewRequest,
    ) -> Result<ViewOpenResponse> {
        let request = OpenTimelineViewRequest {
            conversation_id: request.conversation_id.trim().to_string(),
            message_limit: normalized_message_limit(request.message_limit),
        };
        let snapshot = self.timeline_snapshot(&request).await?;
        let (view_id, order) = self.next_view_id("timeline");
        self.inner.views.write().await.insert_timeline(
            view_id.clone(),
            request,
            order,
            snapshot.clone(),
        );
        Ok(ViewOpenResponse {
            view_id,
            snapshot: ViewSnapshot::Timeline(snapshot),
        })
    }

    pub async fn load_older_timeline(
        &self,
        request: LoadOlderTimelineViewRequest,
    ) -> Result<ViewLoadOlderResponse> {
        let view_id = request.view_id.trim().to_string();
        if view_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "viewId is required",
            ));
        }
        let requested_limit = normalized_message_limit(request.message_limit) as usize;
        let (conversation_id, before_seq, page_limit) = {
            let views = self.inner.views.read().await;
            let registration = views.timelines.get(&view_id).ok_or_else(|| {
                FlareError::localized(ErrorCode::InvalidParameter, "timeline view not found")
            })?;
            let current_len = registration.last_snapshot.messages.len();
            if current_len >= MAX_TIMELINE_WINDOW_MESSAGES {
                return Ok(ViewLoadOlderResponse {
                    view_id,
                    loaded_count: 0,
                    has_more: registration.last_snapshot.has_more,
                    update: None,
                });
            }
            let before_seq = oldest_positive_seq(&registration.last_snapshot);
            let Some(before_seq) = before_seq else {
                return Ok(ViewLoadOlderResponse {
                    view_id,
                    loaded_count: 0,
                    has_more: false,
                    update: None,
                });
            };
            (
                registration.request.conversation_id.clone(),
                before_seq,
                requested_limit.min(MAX_TIMELINE_WINDOW_MESSAGES - current_len),
            )
        };

        if page_limit == 0 {
            return Ok(ViewLoadOlderResponse {
                view_id,
                loaded_count: 0,
                has_more: true,
                update: None,
            });
        }

        let older = self
            .inner
            .conversation_api
            .timeline_page(&conversation_id, before_seq, page_limit as u32)
            .await?;

        let (loaded_count, has_more, update) = {
            let mut views = self.inner.views.write().await;
            let registration = views.timelines.get_mut(&view_id).ok_or_else(|| {
                FlareError::localized(ErrorCode::InvalidParameter, "timeline view not found")
            })?;
            let old = registration.last_snapshot.clone();
            let page_was_full = older.len() >= page_limit;
            let (new, loaded_count) =
                expand_timeline_snapshot(&old, &older, page_was_full, MAX_TIMELINE_WINDOW_MESSAGES);
            let has_more = new.has_more;
            let update = timeline_view_update(&view_id, &old, &new);
            registration.request.message_limit = registration
                .request
                .message_limit
                .max(new.messages.len() as u32);
            registration.last_snapshot = new;
            (loaded_count, has_more, update)
        };

        Ok(ViewLoadOlderResponse {
            view_id,
            loaded_count,
            has_more,
            update,
        })
    }

    pub async fn open_conversation_list(
        &self,
        request: OpenConversationListViewRequest,
    ) -> Result<ViewOpenResponse> {
        let request = OpenConversationListViewRequest {
            conversation_limit: normalized_conversation_limit(request.conversation_limit),
        };
        let snapshot = self.conversation_list_snapshot(&request).await?;
        let (view_id, order) = self.next_view_id("conversation_list");
        self.inner.views.write().await.insert_conversation_list(
            view_id.clone(),
            request,
            order,
            snapshot.clone(),
        );
        Ok(ViewOpenResponse {
            view_id,
            snapshot: ViewSnapshot::ConversationList(snapshot),
        })
    }

    pub async fn close(&self, request: CloseViewRequest) -> Result<CloseViewResponse> {
        let view_id = request.view_id.trim();
        if view_id.is_empty() {
            return Ok(CloseViewResponse { closed: false });
        }
        let mut views = self.inner.views.write().await;
        let closed = views.timelines.remove(view_id).is_some()
            || views.conversation_lists.remove(view_id).is_some();
        Ok(CloseViewResponse { closed })
    }

    async fn timeline_snapshot(
        &self,
        request: &OpenTimelineViewRequest,
    ) -> Result<crate::model::ConversationTimelineSnapshot> {
        self.inner
            .conversation_api
            .open_timeline(OpenConversationTimelineRequest {
                conversation_id: request.conversation_id.clone(),
                message_limit: request.message_limit,
            })
            .await
    }

    async fn conversation_list_snapshot(
        &self,
        request: &OpenConversationListViewRequest,
    ) -> Result<crate::model::HomeTimelineSnapshot> {
        self.inner
            .conversation_api
            .bootstrap_home(BootstrapHomeTimelineRequest {
                conversation_limit: request.conversation_limit,
            })
            .await
    }

    fn next_view_id(&self, prefix: &str) -> (String, u64) {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        (format!("{prefix}:{id}"), id)
    }

    fn spawn_refresh_worker(&self) -> BackgroundTask {
        let inner = Arc::downgrade(&self.inner);
        let bus = self.inner.bus.clone();
        spawn_background_task(async move {
            let mut rx = bus.subscribe_raw();
            loop {
                let mut plan = match rx.recv().await {
                    Ok(event) => {
                        let Some(plan) = view_refresh_plan(&event) else {
                            continue;
                        };
                        plan
                    }
                    Err(_) => break,
                };
                delay(VIEW_REFRESH_DEBOUNCE).await;

                let mut closed = false;
                loop {
                    match rx.try_recv() {
                        Ok(event) => {
                            if let Some(next_plan) = view_refresh_plan(&event) {
                                plan.merge(next_plan);
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            closed = true;
                            break;
                        }
                    }
                }

                let Some(inner) = inner.upgrade() else {
                    break;
                };
                let api = ViewApi { inner };
                api.refresh_open_views(plan).await;
                if closed {
                    break;
                }
            }
        })
    }

    async fn refresh_open_views(&self, plan: ViewRefreshPlan) {
        if !plan.hot_messages.is_empty() {
            self.publish_hot_timeline_messages(&plan.hot_messages, plan.timeline_query.as_ref())
                .await;
        }

        let (timelines, conversation_lists) = {
            let views = self.inner.views.read().await;
            (
                plan.timeline_query
                    .as_ref()
                    .map(|target| {
                        views
                            .timelines
                            .iter()
                            .filter(|(_, view)| {
                                target.matches_timeline(&view.request.conversation_id)
                            })
                            .map(|(view_id, view)| (view_id.clone(), view.request.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                if plan.refresh_conversation_lists {
                    views
                        .conversation_lists
                        .iter()
                        .map(|(view_id, view)| (view_id.clone(), view.request.clone()))
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                },
            )
        };

        for (view_id, request) in timelines {
            match self.timeline_snapshot(&request).await {
                Ok(snapshot) => self.publish_timeline_refresh(view_id, snapshot).await,
                Err(error) => tracing::warn!(
                    target = "flare_sdk.view",
                    view_id,
                    error = %error,
                    "refresh timeline view failed"
                ),
            }
        }

        for (view_id, request) in conversation_lists {
            match self.conversation_list_snapshot(&request).await {
                Ok(snapshot) => {
                    self.publish_conversation_list_refresh(view_id, snapshot)
                        .await
                }
                Err(error) => tracing::warn!(
                    target = "flare_sdk.view",
                    view_id,
                    error = %error,
                    "refresh conversation list view failed"
                ),
            }
        }
    }

    async fn publish_hot_timeline_messages(
        &self,
        messages: &[IMMessage],
        query_target: Option<&ViewRefreshTarget>,
    ) {
        let active_conversation_ids = {
            let views = self.inner.views.read().await;
            views
                .timelines
                .values()
                .filter(|view| {
                    !query_target
                        .map(|target| target.matches_timeline(&view.request.conversation_id))
                        .unwrap_or(false)
                })
                .map(|view| view.request.conversation_id.clone())
                .collect::<HashSet<_>>()
        };
        if active_conversation_ids.is_empty() {
            return;
        }

        let mut hot_messages = messages
            .iter()
            .filter(|message| active_conversation_ids.contains(message.conversation_id.trim()))
            .cloned()
            .collect::<Vec<_>>();
        if hot_messages.is_empty() {
            return;
        }

        let mut latest_conversations = HashMap::new();
        for conversation_id in &active_conversation_ids {
            if !hot_messages
                .iter()
                .any(|message| message.conversation_id.trim() == conversation_id)
            {
                continue;
            }
            match self.inner.conversation_api.get(conversation_id).await {
                Ok(Some(conversation)) => {
                    latest_conversations.insert(conversation_id.clone(), conversation);
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    target = "flare_sdk.view",
                    conversation_id,
                    error = %error,
                    "load hot conversation projection failed"
                ),
            }
        }

        if let Err(error) = self
            .inner
            .conversation_api
            .hydrate_timeline_messages(&mut hot_messages)
            .await
        {
            tracing::warn!(
                target = "flare_sdk.view",
                error = %error,
                "hydrate hot timeline messages failed"
            );
            return;
        }

        let updates = {
            let mut views = self.inner.views.write().await;
            let mut updates = Vec::new();
            for (view_id, registration) in views.timelines.iter_mut() {
                if query_target
                    .map(|target| target.matches_timeline(&registration.request.conversation_id))
                    .unwrap_or(false)
                {
                    continue;
                }
                let old = registration.last_snapshot.clone();
                let Some(new) = hot_timeline_snapshot(
                    &old,
                    &hot_messages,
                    registration.request.message_limit,
                    latest_conversations.get(&registration.request.conversation_id),
                ) else {
                    continue;
                };
                let update = timeline_view_update(view_id, &old, &new);
                registration.last_snapshot = new;
                if let Some(update) = update {
                    updates.push(update);
                }
            }
            updates
        };

        for update in updates {
            self.inner.bus.publish(SdkEvent::View(update));
        }
    }

    async fn publish_timeline_refresh(
        &self,
        view_id: String,
        snapshot: ConversationTimelineSnapshot,
    ) {
        let update = {
            let mut views = self.inner.views.write().await;
            let Some(registration) = views.timelines.get_mut(&view_id) else {
                return;
            };
            let update = timeline_view_update(&view_id, &registration.last_snapshot, &snapshot);
            registration.last_snapshot = snapshot;
            update
        };
        if let Some(update) = update {
            self.inner.bus.publish(SdkEvent::View(update));
        }
    }

    async fn publish_conversation_list_refresh(
        &self,
        view_id: String,
        snapshot: HomeTimelineSnapshot,
    ) {
        let update = {
            let mut views = self.inner.views.write().await;
            let Some(registration) = views.conversation_lists.get_mut(&view_id) else {
                return;
            };
            let update =
                conversation_list_view_update(&view_id, &registration.last_snapshot, &snapshot);
            registration.last_snapshot = snapshot;
            update
        };
        if let Some(update) = update {
            self.inner.bus.publish(SdkEvent::View(update));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ViewRefreshTarget {
    All,
    Conversations(HashSet<String>),
}

impl ViewRefreshTarget {
    fn matches_timeline(&self, conversation_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Conversations(ids) => ids.contains(conversation_id),
        }
    }

    fn merge(&mut self, other: ViewRefreshTarget) {
        if matches!(self, Self::All) {
            return;
        }
        match other {
            Self::All => *self = Self::All,
            Self::Conversations(other_ids) => {
                if let Self::Conversations(ids) = self {
                    ids.extend(other_ids);
                }
            }
        }
    }
}

struct ViewRefreshPlan {
    timeline_query: Option<ViewRefreshTarget>,
    hot_messages: Vec<IMMessage>,
    refresh_conversation_lists: bool,
}

impl ViewRefreshPlan {
    fn query(target: ViewRefreshTarget) -> Self {
        Self {
            timeline_query: Some(target),
            hot_messages: Vec::new(),
            refresh_conversation_lists: true,
        }
    }

    fn hot_messages(messages: Vec<IMMessage>) -> Option<Self> {
        (!messages.is_empty()).then_some(Self {
            timeline_query: None,
            hot_messages: messages,
            refresh_conversation_lists: true,
        })
    }

    fn merge(&mut self, other: ViewRefreshPlan) {
        match (&mut self.timeline_query, other.timeline_query) {
            (Some(target), Some(other_target)) => target.merge(other_target),
            (None, Some(other_target)) => self.timeline_query = Some(other_target),
            _ => {}
        }
        self.hot_messages.extend(other.hot_messages);
        self.refresh_conversation_lists |= other.refresh_conversation_lists;
    }
}

fn view_refresh_plan(event: &SdkEvent) -> Option<ViewRefreshPlan> {
    match event {
        SdkEvent::Message(MessageEvent::Received { message }) => {
            ViewRefreshPlan::hot_messages(vec![message.as_ref().clone()])
        }
        SdkEvent::Message(MessageEvent::ReceivedBatch { messages }) => {
            ViewRefreshPlan::hot_messages(messages.clone())
        }
        _ => view_refresh_target(event).map(ViewRefreshPlan::query),
    }
}

fn view_refresh_target(event: &SdkEvent) -> Option<ViewRefreshTarget> {
    match event {
        SdkEvent::Message(MessageEvent::Received { .. } | MessageEvent::ReceivedBatch { .. }) => {
            None
        }
        SdkEvent::Message(event) => Some(message_refresh_target(event)),
        SdkEvent::Conversation(event) => Some(conversation_refresh_target(event)),
        SdkEvent::Sync(SyncNotify::Finished { .. } | SyncNotify::ResyncNeeded { .. }) => {
            Some(ViewRefreshTarget::All)
        }
        _ => None,
    }
}

fn message_refresh_target(event: &MessageEvent) -> ViewRefreshTarget {
    let mut ids = HashSet::new();
    match event {
        MessageEvent::Received { message } => insert_non_empty(&mut ids, &message.conversation_id),
        MessageEvent::ReceivedBatch { messages } => {
            for message in messages {
                insert_non_empty(&mut ids, &message.conversation_id);
            }
        }
        MessageEvent::SendAck { ack } => insert_non_empty(&mut ids, &ack.conversation_id),
        MessageEvent::Recalled {
            conversation_id, ..
        }
        | MessageEvent::Typing {
            conversation_id, ..
        }
        | MessageEvent::Edited {
            conversation_id, ..
        }
        | MessageEvent::ReactionChanged {
            conversation_id, ..
        }
        | MessageEvent::Deleted {
            conversation_id, ..
        }
        | MessageEvent::ReadReceipt {
            conversation_id, ..
        }
        | MessageEvent::RetentionScheduled {
            conversation_id, ..
        }
        | MessageEvent::RetentionExpired {
            conversation_id, ..
        }
        | MessageEvent::RetentionPurged {
            conversation_id, ..
        }
        | MessageEvent::Pinned {
            conversation_id, ..
        }
        | MessageEvent::Unpinned {
            conversation_id, ..
        }
        | MessageEvent::Marked {
            conversation_id, ..
        }
        | MessageEvent::Unmarked {
            conversation_id, ..
        }
        | MessageEvent::PresenceChanged {
            conversation_id, ..
        }
        | MessageEvent::Capability {
            conversation_id, ..
        }
        | MessageEvent::Custom {
            conversation_id, ..
        } => insert_non_empty(&mut ids, conversation_id),
        MessageEvent::SendFailed { .. } => return ViewRefreshTarget::All,
    }
    if ids.is_empty() {
        ViewRefreshTarget::All
    } else {
        ViewRefreshTarget::Conversations(ids)
    }
}

fn conversation_refresh_target(event: &ConversationEvent) -> ViewRefreshTarget {
    let mut ids = HashSet::new();
    match event {
        ConversationEvent::Synced { conversation_ids } => {
            for conversation_id in conversation_ids {
                insert_non_empty(&mut ids, conversation_id);
            }
        }
        ConversationEvent::Created { conversation_id }
        | ConversationEvent::Updated { conversation_id }
        | ConversationEvent::UnreadCountChanged {
            conversation_id, ..
        }
        | ConversationEvent::Deleted { conversation_id } => {
            insert_non_empty(&mut ids, conversation_id)
        }
    }
    if ids.is_empty() {
        ViewRefreshTarget::All
    } else {
        ViewRefreshTarget::Conversations(ids)
    }
}

fn insert_non_empty(ids: &mut HashSet<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        ids.insert(value.to_string());
    }
}

fn hot_timeline_snapshot(
    snapshot: &ConversationTimelineSnapshot,
    incoming: &[IMMessage],
    limit: u32,
    latest_conversation: Option<&Conversation>,
) -> Option<ConversationTimelineSnapshot> {
    let conversation_id = snapshot
        .conversation
        .as_ref()
        .map(|conversation| conversation.conversation_id.trim())
        .filter(|id| !id.is_empty())
        .or_else(|| {
            snapshot
                .messages
                .first()
                .map(|message| message.conversation_id.trim())
                .filter(|id| !id.is_empty())
        })?;
    let mut messages = snapshot.messages.clone();
    let mut changed = false;

    for message in incoming
        .iter()
        .filter(|message| message.conversation_id.trim() == conversation_id)
    {
        let key = message.timeline_key();
        if let Some(existing) = messages
            .iter_mut()
            .find(|existing| existing.timeline_key() == key)
        {
            if json_changed(&*existing, message) {
                *existing = message.clone();
                changed = true;
            }
        } else {
            messages.push(message.clone());
            changed = true;
        }
    }

    if !changed {
        return None;
    }

    messages.sort_by(IMMessage::compare_for_latest_window_desc);
    let limit = normalized_message_limit(limit) as usize;
    let truncated = messages.len() > limit;
    messages.truncate(limit);
    messages.sort_by(IMMessage::compare_for_timeline_asc);

    Some(ConversationTimelineSnapshot {
        conversation: latest_conversation
            .cloned()
            .or_else(|| snapshot.conversation.clone()),
        messages,
        has_more: snapshot.has_more || truncated,
    })
}

fn oldest_positive_seq(snapshot: &ConversationTimelineSnapshot) -> Option<u64> {
    snapshot
        .messages
        .iter()
        .filter_map(|message| (message.conversation_seq > 0).then_some(message.conversation_seq))
        .min()
}

fn expand_timeline_snapshot(
    snapshot: &ConversationTimelineSnapshot,
    older: &[IMMessage],
    page_was_full: bool,
    max_window: usize,
) -> (ConversationTimelineSnapshot, u32) {
    let mut messages = snapshot.messages.clone();
    let mut keys = messages
        .iter()
        .map(|message| message.timeline_key())
        .collect::<HashSet<_>>();
    let mut loaded_count = 0_u32;
    for message in older {
        if keys.insert(message.timeline_key()) {
            messages.push(message.clone());
            loaded_count += 1;
        }
    }
    messages.sort_by(IMMessage::compare_for_latest_window_desc);
    messages.truncate(max_window);
    let has_more = page_was_full && oldest_positive_seq_for_messages(&messages).unwrap_or(0) > 1;
    messages.sort_by(IMMessage::compare_for_timeline_asc);
    (
        ConversationTimelineSnapshot {
            conversation: snapshot.conversation.clone(),
            messages,
            has_more,
        },
        loaded_count,
    )
}

fn oldest_positive_seq_for_messages(messages: &[IMMessage]) -> Option<u64> {
    messages
        .iter()
        .filter_map(|message| (message.conversation_seq > 0).then_some(message.conversation_seq))
        .min()
}

fn timeline_view_update(
    view_id: &str,
    old: &ConversationTimelineSnapshot,
    new: &ConversationTimelineSnapshot,
) -> Option<ViewUpdate> {
    let Some(ops) = keyed_delta_ops(&old.messages, &new.messages, |message| {
        message.timeline_key().trim().to_string()
    }) else {
        return Some(snapshot_update(
            view_id,
            ViewSnapshot::Timeline(new.clone()),
        ));
    };
    let side_changed =
        json_changed(&old.conversation, &new.conversation) || old.has_more != new.has_more;
    if ops.is_empty() && !side_changed {
        return None;
    }
    Some(ViewUpdate {
        view_id: view_id.to_string(),
        kind: ViewUpdateKind::Delta,
        snapshot: None,
        delta: Some(ViewDelta::Timeline {
            ops,
            conversation: new.conversation.clone(),
            has_more: new.has_more,
        }),
    })
}

fn conversation_list_view_update(
    view_id: &str,
    old: &HomeTimelineSnapshot,
    new: &HomeTimelineSnapshot,
) -> Option<ViewUpdate> {
    let Some(ops) = keyed_delta_ops(&old.conversations, &new.conversations, |conversation| {
        conversation.conversation_id.trim().to_string()
    }) else {
        return Some(snapshot_update(
            view_id,
            ViewSnapshot::ConversationList(new.clone()),
        ));
    };
    let side_changed =
        old.total_unread != new.total_unread || json_changed(&old.sync_state, &new.sync_state);
    if ops.is_empty() && !side_changed {
        return None;
    }
    Some(ViewUpdate {
        view_id: view_id.to_string(),
        kind: ViewUpdateKind::Delta,
        snapshot: None,
        delta: Some(ViewDelta::ConversationList {
            ops,
            total_unread: new.total_unread,
            sync_state: new.sync_state.clone(),
        }),
    })
}

fn snapshot_update(view_id: &str, snapshot: ViewSnapshot) -> ViewUpdate {
    ViewUpdate {
        view_id: view_id.to_string(),
        kind: ViewUpdateKind::Snapshot,
        snapshot: Some(snapshot),
        delta: None,
    }
}

fn keyed_delta_ops<T, K>(old: &[T], new: &[T], key: K) -> Option<Vec<ViewDeltaOp>>
where
    T: Serialize,
    K: Fn(&T) -> String,
{
    let mut old_by_key: HashMap<String, (usize, Value)> = HashMap::with_capacity(old.len());
    for (index, item) in old.iter().enumerate() {
        let key = key(item);
        if key.is_empty() {
            return None;
        }
        let value = serde_json::to_value(item).ok()?;
        old_by_key.insert(key, (index, value));
    }

    let mut new_by_key = HashSet::with_capacity(new.len());
    let mut ops = Vec::new();
    for (index, item) in new.iter().enumerate() {
        let key = key(item);
        if key.is_empty() {
            return None;
        }
        let value = serde_json::to_value(item).ok()?;
        new_by_key.insert(key.clone());
        match old_by_key.get(&key) {
            None => ops.push(ViewDeltaOp {
                op: ViewDeltaKind::Insert,
                key,
                index: index as u32,
                from_index: None,
                item: Some(value),
            }),
            Some((old_index, old_value)) => {
                if *old_index != index {
                    ops.push(ViewDeltaOp {
                        op: ViewDeltaKind::Move,
                        key: key.clone(),
                        index: index as u32,
                        from_index: Some(*old_index as u32),
                        item: None,
                    });
                }
                if old_value != &value {
                    ops.push(ViewDeltaOp {
                        op: ViewDeltaKind::Update,
                        key,
                        index: index as u32,
                        from_index: None,
                        item: Some(value),
                    });
                }
            }
        }
    }

    for (key, (index, _)) in old_by_key {
        if !new_by_key.contains(&key) {
            ops.push(ViewDeltaOp {
                op: ViewDeltaKind::Remove,
                key,
                index: index as u32,
                from_index: None,
                item: None,
            });
        }
    }

    Some(ops)
}

fn json_changed<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_value(left).ok() != serde_json::to_value(right).ok()
}

#[cfg(test)]
mod tests {
    use flare_proto::common::{Message as ProtoMessage, SendAck};

    use super::{
        ViewRefreshTarget, ViewRegistrations, expand_timeline_snapshot, hot_timeline_snapshot,
        oldest_positive_seq, timeline_view_update, view_refresh_plan, view_refresh_target,
    };
    use crate::kernel::event::{MessageEvent, SdkEvent, SyncNotify, SyncPhase};
    use crate::kernel::{SyncReason, SyncRunContext, SyncScope, SyncTrigger, SyncVisibility};
    use crate::model::{
        Conversation, ConversationTimelineSnapshot, IMMessage, OpenTimelineViewRequest, ViewDelta,
        ViewDeltaKind, ViewUpdateKind,
    };

    #[test]
    fn message_batch_refresh_plan_uses_hot_messages_without_timeline_query() {
        let event = SdkEvent::Message(MessageEvent::ReceivedBatch {
            messages: vec![
                IMMessage::new(ProtoMessage {
                    conversation_id: "c1".into(),
                    ..Default::default()
                }),
                IMMessage::new(ProtoMessage {
                    conversation_id: "c2".into(),
                    ..Default::default()
                }),
                IMMessage::new(ProtoMessage {
                    conversation_id: "c1".into(),
                    ..Default::default()
                }),
            ],
        });
        let plan = view_refresh_plan(&event).expect("message event refreshes views");
        assert!(plan.timeline_query.is_none());
        assert!(plan.refresh_conversation_lists);
        assert_eq!(plan.hot_messages.len(), 3);
    }

    #[test]
    fn send_ack_refresh_uses_ack_conversation_id() {
        let event = SdkEvent::Message(MessageEvent::SendAck {
            ack: Box::new(SendAck {
                conversation_id: "c1".into(),
                ..Default::default()
            }),
        });
        let target = view_refresh_target(&event).expect("ack refreshes views");
        assert!(target.matches_timeline("c1"));
        assert!(!target.matches_timeline("c2"));
    }

    #[test]
    fn sync_finish_refreshes_all_views() {
        let event = SdkEvent::Sync(SyncNotify::Finished {
            run: SyncRunContext::new(
                SyncTrigger::Manual,
                SyncScope::Global,
                SyncVisibility::Silent,
                SyncReason::UserRequested,
            ),
            phase: SyncPhase::Background,
        });
        assert_eq!(view_refresh_target(&event), Some(ViewRefreshTarget::All));
    }

    #[test]
    fn refresh_target_merge_unions_conversation_targets() {
        let mut target = ViewRefreshTarget::Conversations(["c1".to_string()].into_iter().collect());

        target.merge(ViewRefreshTarget::Conversations(
            ["c1".to_string(), "c2".to_string()].into_iter().collect(),
        ));

        assert!(target.matches_timeline("c1"));
        assert!(target.matches_timeline("c2"));
        assert!(!target.matches_timeline("c3"));
    }

    #[test]
    fn refresh_target_merge_all_dominates() {
        let mut target = ViewRefreshTarget::Conversations(["c1".to_string()].into_iter().collect());
        target.merge(ViewRefreshTarget::All);
        assert_eq!(target, ViewRefreshTarget::All);

        target.merge(ViewRefreshTarget::Conversations(
            ["c2".to_string()].into_iter().collect(),
        ));
        assert_eq!(target, ViewRefreshTarget::All);
    }

    #[test]
    fn timeline_registry_replaces_same_conversation_and_caps_oldest() {
        let mut views = ViewRegistrations::default();
        for index in 0..9 {
            views.insert_timeline(
                format!("timeline:{index}"),
                OpenTimelineViewRequest {
                    conversation_id: format!("c{index}"),
                    message_limit: 50,
                },
                index,
                empty_timeline_snapshot(),
            );
        }

        assert_eq!(views.timelines.len(), 8);
        assert!(!views.timelines.contains_key("timeline:0"));

        views.insert_timeline(
            "timeline:replacement".to_string(),
            OpenTimelineViewRequest {
                conversation_id: "c8".to_string(),
                message_limit: 50,
            },
            10,
            empty_timeline_snapshot(),
        );

        assert_eq!(views.timelines.len(), 8);
        assert!(!views.timelines.contains_key("timeline:8"));
        assert!(views.timelines.contains_key("timeline:replacement"));
    }

    #[test]
    fn timeline_delta_inserts_only_new_message() {
        let old = ConversationTimelineSnapshot {
            conversation: None,
            messages: vec![message("c1", "m1", 1)],
            has_more: false,
        };
        let new = ConversationTimelineSnapshot {
            conversation: None,
            messages: vec![message("c1", "m1", 1), message("c1", "m2", 2)],
            has_more: false,
        };

        let Some(update) = timeline_view_update("timeline:1", &old, &new) else {
            panic!("new message should produce a view update");
        };
        assert_eq!(update.kind, ViewUpdateKind::Delta);
        assert!(update.snapshot.is_none());
        let Some(ViewDelta::Timeline { ops, .. }) = update.delta else {
            panic!("timeline delta expected");
        };
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].op, ViewDeltaKind::Insert);
        assert_eq!(ops[0].key, "client:m2");
        assert_eq!(ops[0].index, 1);
        assert!(ops[0].item.is_some());
    }

    #[test]
    fn hot_timeline_snapshot_keeps_latest_window_order_and_limit() {
        let old = ConversationTimelineSnapshot {
            conversation: None,
            messages: vec![message("c1", "m2", 2), message("c1", "m1", 1)],
            has_more: false,
        };

        let new = hot_timeline_snapshot(&old, &[message("c1", "m3", 3)], 2, None)
            .expect("new message should update hot timeline");

        assert_eq!(new.messages.len(), 2);
        assert_eq!(new.messages[0].timeline_key(), "client:m2");
        assert_eq!(new.messages[1].timeline_key(), "client:m3");
        assert!(new.has_more);
    }

    #[test]
    fn hot_timeline_snapshot_carries_latest_conversation_projection() {
        let old = ConversationTimelineSnapshot {
            conversation: Some(Conversation {
                conversation_id: "c1".to_string(),
                last_message_preview: None,
                ..Default::default()
            }),
            messages: vec![message("c1", "m1", 1)],
            has_more: false,
        };
        let latest = Conversation {
            conversation_id: "c1".to_string(),
            last_message_preview: Some("hello".to_string()),
            ..Default::default()
        };

        let new = hot_timeline_snapshot(&old, &[message("c1", "m2", 2)], 50, Some(&latest))
            .expect("new message should update hot timeline");

        assert_eq!(
            new.conversation
                .as_ref()
                .and_then(|conversation| conversation.last_message_preview.as_deref()),
            Some("hello")
        );
    }

    #[test]
    fn expand_timeline_snapshot_appends_older_page_without_duplicates() {
        let old = ConversationTimelineSnapshot {
            conversation: None,
            messages: vec![message("c1", "m3", 3), message("c1", "m2", 2)],
            has_more: true,
        };

        let (new, loaded_count) = expand_timeline_snapshot(
            &old,
            &[message("c1", "m2", 2), message("c1", "m1", 1)],
            false,
            500,
        );

        assert_eq!(loaded_count, 1);
        assert_eq!(
            new.messages
                .iter()
                .map(IMMessage::timeline_key)
                .collect::<Vec<_>>(),
            vec!["client:m1", "client:m2", "client:m3"]
        );
        assert!(!new.has_more);
    }

    #[test]
    fn oldest_positive_seq_ignores_local_pending_messages() {
        let snapshot = ConversationTimelineSnapshot {
            conversation: None,
            messages: vec![message("c1", "pending", 0), message("c1", "m4", 4)],
            has_more: true,
        };

        assert_eq!(oldest_positive_seq(&snapshot), Some(4));
    }

    fn empty_timeline_snapshot() -> ConversationTimelineSnapshot {
        ConversationTimelineSnapshot {
            conversation: None,
            messages: Vec::new(),
            has_more: false,
        }
    }

    fn message(conversation_id: &str, client_msg_id: &str, seq: u64) -> IMMessage {
        IMMessage::new(ProtoMessage {
            conversation_id: conversation_id.into(),
            client_msg_id: client_msg_id.into(),
            conversation_seq: seq,
            ..Default::default()
        })
    }
}
