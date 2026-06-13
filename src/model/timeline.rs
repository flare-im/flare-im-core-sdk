use serde::{Deserialize, Serialize};

use crate::model::{Conversation, IMMessage};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimelineSyncState {
    LocalReady,
    Synced,
    Partial,
}

impl Default for TimelineSyncState {
    fn default() -> Self {
        Self::LocalReady
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeTimelineSnapshot {
    pub conversations: Vec<Conversation>,
    pub total_unread: u64,
    pub sync_state: TimelineSyncState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationTimelineSnapshot {
    pub conversation: Option<Conversation>,
    pub messages: Vec<IMMessage>,
    pub has_more: bool,
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
