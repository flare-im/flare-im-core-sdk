use crate::domain::{
    ConversationCursorSelection, CriticalEventQueryPlan, DeleteVisibilityDecision,
};
use crate::model::event::EventType;

pub struct SyncPolicy;

impl SyncPolicy {
    pub fn critical_event_query_plan() -> CriticalEventQueryPlan {
        CriticalEventQueryPlan {
            event_types: vec![
                EventType::EventMessageRecall as i32,
                EventType::EventMessageEdit as i32,
                EventType::EventMessageDelete as i32,
                EventType::EventReaction as i32,
                EventType::EventPin as i32,
                EventType::EventUnpin as i32,
                EventType::EventMark as i32,
                EventType::EventUnmark as i32,
            ],
        }
    }

    pub fn evaluate_delete_visibility(
        user_id: &str,
        scope: i32,
        target_user_id: Option<&str>,
    ) -> DeleteVisibilityDecision {
        if scope != 1 {
            return DeleteVisibilityDecision {
                apply_to_current_user: true,
            };
        }
        let target_user_id = target_user_id.unwrap_or_default();
        if target_user_id.is_empty() {
            return DeleteVisibilityDecision {
                apply_to_current_user: true,
            };
        }
        DeleteVisibilityDecision {
            apply_to_current_user: !user_id.is_empty() && user_id == target_user_id,
        }
    }

    pub fn select_conversation_cursor_ms(
        local_cursor_ms: Option<u64>,
        remote_cursor_ms: Option<u64>,
        local_conversation_count: usize,
    ) -> ConversationCursorSelection {
        if local_conversation_count == 0 {
            return ConversationCursorSelection {
                selected_cursor_ms: None,
                drop_local_incremental_cursor: false,
                force_full_sync: true,
            };
        }

        match (local_cursor_ms, remote_cursor_ms) {
            (Some(local), Some(remote)) => ConversationCursorSelection {
                selected_cursor_ms: Some(local.max(remote)),
                drop_local_incremental_cursor: false,
                force_full_sync: false,
            },
            (Some(_), None) => ConversationCursorSelection {
                selected_cursor_ms: None,
                drop_local_incremental_cursor: true,
                force_full_sync: false,
            },
            (None, Some(remote)) => ConversationCursorSelection {
                selected_cursor_ms: Some(remote),
                drop_local_incremental_cursor: false,
                force_full_sync: false,
            },
            (None, None) => ConversationCursorSelection {
                selected_cursor_ms: None,
                drop_local_incremental_cursor: false,
                force_full_sync: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SyncPolicy;

    #[test]
    fn delete_visibility_respects_private_target_user() {
        let visible = SyncPolicy::evaluate_delete_visibility("u1", 1, Some("u1"));
        let hidden = SyncPolicy::evaluate_delete_visibility("u2", 1, Some("u1"));

        assert!(visible.apply_to_current_user);
        assert!(!hidden.apply_to_current_user);
    }

    #[test]
    fn cursor_selection_forces_full_sync_when_local_empty() {
        let decision = SyncPolicy::select_conversation_cursor_ms(Some(10), Some(20), 0);

        assert_eq!(decision.selected_cursor_ms, None);
        assert!(!decision.drop_local_incremental_cursor);
        assert!(decision.force_full_sync);
    }

    #[test]
    fn cursor_selection_drops_local_incremental_when_remote_missing() {
        let decision = SyncPolicy::select_conversation_cursor_ms(Some(10), None, 5);

        assert_eq!(decision.selected_cursor_ms, None);
        assert!(decision.drop_local_incremental_cursor);
        assert!(!decision.force_full_sync);
    }
}
