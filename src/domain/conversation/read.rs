use crate::domain::ConversationReadDecision;
use crate::model::Conversation;

pub struct ConversationReadService;

impl ConversationReadService {
    pub fn plan_mark_read(
        &self,
        current: &Conversation,
        local_max_seq: u64,
        requested_read_seq: u64,
    ) -> ConversationReadDecision {
        let requested_read_seq = if requested_read_seq == 0 {
            local_max_seq.max(current.max_seq)
        } else {
            requested_read_seq
        };
        let next_read_seq = if requested_read_seq == 0 {
            requested_read_seq.max(current.last_read_seq)
        } else {
            requested_read_seq
                .max(current.last_read_seq)
                .min(current.max_seq)
        };
        let unread_count = if next_read_seq >= current.max_seq {
            0
        } else {
            current.unread_count
        };
        ConversationReadDecision {
            unread_count,
            next_read_seq,
            should_recompute_local_unread: requested_read_seq != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ConversationReadService;
    use crate::model::Conversation;

    #[test]
    fn mark_read_zero_advances_to_conversation_max_and_clears_unread() {
        let service = ConversationReadService;
        let mut conversation = Conversation::default();
        conversation.max_seq = 50;
        conversation.last_read_seq = 10;
        conversation.unread_count = 5;

        let decision = service.plan_mark_read(&conversation, 80, 0);

        assert_eq!(decision.next_read_seq, 50);
        assert_eq!(decision.unread_count, 0);
        assert!(decision.should_recompute_local_unread);
    }

    #[test]
    fn mark_read_non_zero_is_clamped_by_conversation_max_seq() {
        let service = ConversationReadService;
        let mut conversation = Conversation::default();
        conversation.max_seq = 50;
        conversation.last_read_seq = 10;
        conversation.unread_count = 5;

        let decision = service.plan_mark_read(&conversation, 80, 999);

        assert_eq!(decision.next_read_seq, 50);
        assert_eq!(decision.unread_count, 0);
        assert!(decision.should_recompute_local_unread);
    }
}
