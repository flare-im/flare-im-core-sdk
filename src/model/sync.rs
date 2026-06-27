use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}
