use std::collections::HashMap;

use crate::domain::MediaProgressiveStage;

use super::{AttentionState, ConvergencePriority, ConvergenceTarget};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaWarmupCandidate {
    pub conversation_id: String,
    pub file_id: String,
    pub message_seq: u64,
    pub desired_stage: MediaProgressiveStage,
    pub estimated_bytes: u64,
}

impl MediaWarmupCandidate {
    pub fn new(
        conversation_id: impl Into<String>,
        file_id: impl Into<String>,
        message_seq: u64,
        desired_stage: MediaProgressiveStage,
        estimated_bytes: u64,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            file_id: file_id.into(),
            message_seq,
            desired_stage,
            estimated_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaWarmupItem {
    pub conversation_id: String,
    pub file_id: String,
    pub message_seq: u64,
    pub desired_stage: MediaProgressiveStage,
    pub estimated_bytes: u64,
    pub priority: ConvergencePriority,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaWarmupPlan {
    pub items: Vec<MediaWarmupItem>,
}

impl MediaWarmupPlan {
    pub fn from_candidates(
        attention: &AttentionState,
        candidates: impl IntoIterator<Item = MediaWarmupCandidate>,
        max_items: usize,
    ) -> Self {
        if max_items == 0 {
            return Self::default();
        }

        let mut best_by_file: HashMap<String, MediaWarmupItem> = HashMap::new();
        for candidate in candidates {
            let priority = attention.priority_for(&ConvergenceTarget::Conversation(
                candidate.conversation_id.clone(),
            ));
            let item = MediaWarmupItem {
                conversation_id: candidate.conversation_id,
                file_id: candidate.file_id,
                message_seq: candidate.message_seq,
                desired_stage: candidate
                    .desired_stage
                    .min(MediaProgressiveStage::LowResolution),
                estimated_bytes: candidate.estimated_bytes,
                priority,
            };

            best_by_file
                .entry(item.file_id.clone())
                .and_modify(|existing| {
                    if Self::is_better(&item, existing) {
                        *existing = item.clone();
                    }
                })
                .or_insert(item);
        }

        let mut items: Vec<_> = best_by_file.into_values().collect();
        items.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| b.message_seq.cmp(&a.message_seq))
                .then_with(|| a.file_id.cmp(&b.file_id))
        });
        items.truncate(max_items);

        Self { items }
    }

    fn is_better(next: &MediaWarmupItem, current: &MediaWarmupItem) -> bool {
        next.priority < current.priority
            || (next.priority == current.priority && next.message_seq > current.message_seq)
    }
}

#[cfg(test)]
mod tests {
    use super::{MediaWarmupCandidate, MediaWarmupPlan};
    use crate::domain::MediaProgressiveStage;
    use crate::kernel::sync::{AttentionState, ConvergencePriority};

    fn candidate(conversation_id: &str, file_id: &str, message_seq: u64) -> MediaWarmupCandidate {
        MediaWarmupCandidate::new(
            conversation_id,
            file_id,
            message_seq,
            MediaProgressiveStage::Thumbnail,
            16_384,
        )
    }

    #[test]
    fn media_warmup_orders_by_attention_then_recent_message() {
        let mut attention = AttentionState::default();
        attention.open_timeline("fg");
        attention.set_visible(["visible"]);
        attention.mark_recent("recent");

        let plan = MediaWarmupPlan::from_candidates(
            &attention,
            vec![
                candidate("cold", "file-cold", 99),
                candidate("visible", "file-visible-old", 1),
                candidate("recent", "file-recent", 100),
                candidate("fg", "file-fg-old", 2),
                candidate("fg", "file-fg-new", 3),
            ],
            5,
        );

        let ids: Vec<_> = plan
            .items
            .iter()
            .map(|item| item.file_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec![
                "file-fg-new",
                "file-fg-old",
                "file-visible-old",
                "file-recent",
                "file-cold"
            ]
        );
        assert_eq!(plan.items[0].priority, ConvergencePriority::P0Foreground);
    }

    #[test]
    fn media_warmup_is_bounded_and_content_deduped() {
        let mut attention = AttentionState::default();
        attention.open_timeline("fg");

        let plan = MediaWarmupPlan::from_candidates(
            &attention,
            vec![
                candidate("fg", "same-content", 1),
                candidate("fg", "same-content", 10),
                candidate("fg", "other-content", 9),
                candidate("cold", "cold-content", 100),
            ],
            2,
        );

        let ids: Vec<_> = plan
            .items
            .iter()
            .map(|item| item.file_id.as_str())
            .collect();
        assert_eq!(ids, vec!["same-content", "other-content"]);
        assert_eq!(plan.items[0].message_seq, 10);
    }
}
