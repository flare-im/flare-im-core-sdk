use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::timeline::HomeTimelineSnapshot;

/// Version stamp used by sync summary reconciliation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct ConversationVersion {
    pub conversation_id: String,
    pub version: u64,
}

/// Request for summary sync with client-known conversation versions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct SyncConversationSummariesRequest {
    pub known_versions: Vec<ConversationVersion>,
}

/// Conversations whose local version is missing or newer than the caller's snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct SyncConversationSummariesResponse {
    pub changed_conversations: Vec<ConversationVersion>,
}

/// Result of direct local-store historical backfill for one conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct ConversationHistoryBackfillResponse {
    pub conversation_id: String,
    pub pages_loaded: u32,
    pub oldest_seq_before: u64,
    pub oldest_seq_after: u64,
    pub has_more: bool,
    pub completed: bool,
}

/// Core-owned startup sync policy shared by all platform SDKs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct StartupHomeSyncRequest {
    pub conversation_limit: u32,
    pub start_background_convergence: bool,
    pub backfill_visible_histories: bool,
    pub history_backfill_limit: i32,
    pub history_backfill_max_pages_per_conversation: u32,
    pub history_backfill_max_conversations: u32,
}

impl Default for StartupHomeSyncRequest {
    fn default() -> Self {
        Self {
            conversation_limit: 100,
            start_background_convergence: true,
            backfill_visible_histories: false,
            history_backfill_limit: 500,
            history_backfill_max_pages_per_conversation: 128,
            history_backfill_max_conversations: 100,
        }
    }
}

/// First usable home snapshot plus diagnostics about the startup sync path.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct StartupHomeSyncResponse {
    pub snapshot: HomeTimelineSnapshot,
    pub served_from_local: bool,
    pub cold_sync_performed: bool,
    pub background_convergence_started: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

impl Default for StartupHomeSyncResponse {
    fn default() -> Self {
        Self {
            snapshot: HomeTimelineSnapshot {
                conversations: Vec::new(),
                total_unread: 0,
                sync_state: crate::model::timeline::TimelineSyncState::LocalReady,
            },
            served_from_local: false,
            cold_sync_performed: false,
            background_convergence_started: false,
            degraded_reason: None,
        }
    }
}

impl SyncConversationSummariesResponse {
    pub fn from_current_versions(
        known_versions: &[ConversationVersion],
        current_versions: impl IntoIterator<Item = ConversationVersion>,
    ) -> Self {
        let mut known: HashMap<&str, u64> = HashMap::new();
        for stamp in known_versions {
            known
                .entry(stamp.conversation_id.as_str())
                .and_modify(|version| *version = (*version).max(stamp.version))
                .or_insert(stamp.version);
        }

        let mut current: BTreeMap<String, u64> = BTreeMap::new();
        for stamp in current_versions {
            current
                .entry(stamp.conversation_id)
                .and_modify(|version| *version = (*version).max(stamp.version))
                .or_insert(stamp.version);
        }

        let changed_conversations = current
            .into_iter()
            .filter_map(|(conversation_id, version)| {
                let known_version = known.get(conversation_id.as_str()).copied();
                if known_version.is_none() || version > known_version.unwrap_or_default() {
                    Some(ConversationVersion {
                        conversation_id,
                        version,
                    })
                } else {
                    None
                }
            })
            .collect();

        Self {
            changed_conversations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(conversation_id: &str, version: u64) -> ConversationVersion {
        ConversationVersion {
            conversation_id: conversation_id.to_string(),
            version,
        }
    }

    #[test]
    fn empty_known_versions_return_all_current_versions() {
        let response = SyncConversationSummariesResponse::from_current_versions(
            &[],
            [stamp("c2", 3), stamp("c1", 1)],
        );

        assert_eq!(
            response.changed_conversations,
            vec![stamp("c1", 1), stamp("c2", 3)]
        );
    }

    #[test]
    fn only_missing_or_newer_versions_are_changed() {
        let response = SyncConversationSummariesResponse::from_current_versions(
            &[stamp("c1", 1), stamp("c2", 10), stamp("c2", 9)],
            [stamp("c1", 2), stamp("c2", 10), stamp("c3", 0)],
        );

        assert_eq!(
            response.changed_conversations,
            vec![stamp("c1", 2), stamp("c3", 0)]
        );
    }

    #[test]
    fn startup_home_sync_request_defaults_to_fast_home_plus_background_convergence() {
        let request = StartupHomeSyncRequest::default();

        assert_eq!(request.conversation_limit, 100);
        assert!(request.start_background_convergence);
        assert!(!request.backfill_visible_histories);
        assert_eq!(request.history_backfill_limit, 500);
        assert_eq!(request.history_backfill_max_pages_per_conversation, 128);
        assert_eq!(request.history_backfill_max_conversations, 100);
    }

    #[test]
    fn startup_home_sync_request_uses_camel_case_wire_fields() {
        let value = serde_json::to_value(StartupHomeSyncRequest::default())
            .expect("startup request serializes");

        assert!(value.get("conversationLimit").is_some());
        assert!(value.get("startBackgroundConvergence").is_some());
        assert!(value.get("backfillVisibleHistories").is_some());
        assert!(
            value
                .get("historyBackfillMaxPagesPerConversation")
                .is_some()
        );
        assert!(value.get("history_backfill_limit").is_none());
    }
}
