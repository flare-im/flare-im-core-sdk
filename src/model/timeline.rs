use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Conversation, IMMessage};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TimelineSyncState {
    #[default]
    LocalReady,
    Synced,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapHomeTimelineRequest {
    #[serde(default = "default_conversation_limit")]
    pub conversation_limit: u32,
}

impl Default for BootstrapHomeTimelineRequest {
    fn default() -> Self {
        Self {
            conversation_limit: default_conversation_limit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenConversationTimelineRequest {
    pub conversation_id: String,
    #[serde(default = "default_message_limit")]
    pub message_limit: u32,
}

impl Default for OpenConversationTimelineRequest {
    fn default() -> Self {
        Self {
            conversation_id: String::new(),
            message_limit: default_message_limit(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HomeTimelineSnapshot {
    pub conversations: Vec<Conversation>,
    pub total_unread: u64,
    pub sync_state: TimelineSyncState,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelineSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Conversation>,
    pub messages: Vec<IMMessage>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenTimelineViewRequest {
    pub conversation_id: String,
    #[serde(default = "default_message_limit")]
    pub message_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoadOlderTimelineViewRequest {
    pub view_id: String,
    #[serde(default = "default_message_limit")]
    pub message_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenConversationListViewRequest {
    #[serde(default = "default_conversation_limit")]
    pub conversation_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloseViewRequest {
    pub view_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "viewType", content = "data", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)]
pub enum ViewSnapshot {
    Timeline(ConversationTimelineSnapshot),
    ConversationList(HomeTimelineSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewOpenResponse {
    pub view_id: String,
    pub snapshot: ViewSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewLoadOlderResponse {
    pub view_id: String,
    pub loaded_count: u32,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<ViewUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ViewUpdateKind {
    Snapshot,
    Delta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ViewDeltaKind {
    Insert,
    Update,
    Remove,
    Move,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewDeltaOp {
    pub op: ViewDeltaKind,
    pub key: String,
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "viewType",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ViewDelta {
    Timeline {
        ops: Vec<ViewDeltaOp>,
        #[serde(skip_serializing_if = "Option::is_none")]
        conversation: Option<Conversation>,
        has_more: bool,
    },
    ConversationList {
        ops: Vec<ViewDeltaOp>,
        total_unread: u64,
        sync_state: TimelineSyncState,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewUpdate {
    pub view_id: String,
    pub kind: ViewUpdateKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<ViewSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<ViewDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloseViewResponse {
    pub closed: bool,
}

fn default_conversation_limit() -> u32 {
    100
}

fn default_message_limit() -> u32 {
    20
}

pub fn normalized_conversation_limit(limit: u32) -> u32 {
    limit.clamp(1, 500)
}

pub fn normalized_message_limit(limit: u32) -> u32 {
    limit.clamp(1, 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn view_update_omits_absent_optional_fields() {
        let update = ViewUpdate {
            view_id: "timeline-1".to_string(),
            kind: ViewUpdateKind::Delta,
            snapshot: None,
            delta: Some(ViewDelta::Timeline {
                ops: vec![ViewDeltaOp {
                    op: ViewDeltaKind::Insert,
                    key: "m1".to_string(),
                    index: 0,
                    from_index: None,
                    item: None,
                }],
                conversation: None,
                has_more: false,
            }),
        };

        let value = serde_json::to_value(update).expect("view update serializes");

        assert_eq!(value["viewId"], json!("timeline-1"));
        assert_eq!(value["kind"], json!("delta"));
        assert!(value.get("snapshot").is_none());

        let delta = value
            .get("delta")
            .and_then(Value::as_object)
            .expect("delta object");
        assert_eq!(delta.get("viewType"), Some(&json!("timeline")));
        assert!(delta.get("conversation").is_none());

        let op = delta
            .get("ops")
            .and_then(Value::as_array)
            .and_then(|ops| ops.first())
            .and_then(Value::as_object)
            .expect("first delta op");
        assert!(op.get("fromIndex").is_none());
        assert!(op.get("item").is_none());
    }

    #[test]
    fn timeline_snapshot_omits_absent_conversation() {
        let snapshot = ConversationTimelineSnapshot {
            conversation: None,
            messages: Vec::new(),
            has_more: false,
        };

        let value = serde_json::to_value(snapshot).expect("timeline snapshot serializes");

        assert!(value.get("conversation").is_none());
        assert_eq!(value["messages"], json!([]));
        assert_eq!(value["hasMore"], json!(false));
    }
}
