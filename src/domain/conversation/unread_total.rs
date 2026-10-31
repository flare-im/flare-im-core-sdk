//! 未读总数：首页快照与 Tab 角标共用的唯一口径。宿主不再自己对会话列表求和。

use crate::model::Conversation;

/// 一个会话计入未读总数的条数：
///
/// - 已归档会话不计入：它不在首页列表里，角标也不该替它叫。
/// - 免打扰会话只计入「@我」的条数（不超过它的未读数）：免打扰挡的是普通消息，
///   点名到我的仍要叫；没有 @ 就一条都不计。列表行里它照旧显示自己的未读数。
/// - 其余会话计入全部未读。
///
/// 「@我」取本地投影的 `mention_count`；只有 `mention_me` 标记而计数为 0 时按 1 条计。
pub fn unread_counted_in_total(conversation: &Conversation) -> u32 {
    if conversation.is_archived {
        return 0;
    }
    if !conversation.is_muted {
        return conversation.unread_count;
    }
    let mentions = conversation
        .mention_count
        .max(u32::from(conversation.mention_me));
    mentions.min(conversation.unread_count)
}

/// 全部会话的未读总数（[`unread_counted_in_total`] 之和）。调用方传入完整列表，
/// 不能是截断后的首屏，否则总数会随首屏条数变化。
pub fn total_unread<'a, I>(conversations: I) -> u64
where
    I: IntoIterator<Item = &'a Conversation>,
{
    conversations
        .into_iter()
        .map(|conversation| u64::from(unread_counted_in_total(conversation)))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation(unread: u32) -> Conversation {
        Conversation {
            conversation_id: format!("c-{unread}"),
            unread_count: unread,
            ..Default::default()
        }
    }

    #[test]
    fn plain_conversations_count_all_their_unread() {
        assert_eq!(total_unread(&[conversation(3), conversation(4)]), 7);
    }

    #[test]
    fn a_muted_conversation_without_mentions_counts_nothing() {
        let muted = Conversation {
            is_muted: true,
            ..conversation(9)
        };
        assert_eq!(unread_counted_in_total(&muted), 0);
        assert_eq!(total_unread(&[muted, conversation(2)]), 2);
    }

    #[test]
    fn a_muted_conversation_counts_only_its_mentions() {
        let muted = Conversation {
            is_muted: true,
            mention_count: 2,
            mention_me: true,
            ..conversation(9)
        };
        assert_eq!(unread_counted_in_total(&muted), 2);
    }

    #[test]
    fn mentions_never_count_beyond_the_unread() {
        let muted = Conversation {
            is_muted: true,
            mention_count: 5,
            ..conversation(1)
        };
        assert_eq!(unread_counted_in_total(&muted), 1);
        let read = Conversation {
            is_muted: true,
            mention_count: 3,
            ..conversation(0)
        };
        assert_eq!(unread_counted_in_total(&read), 0);
    }

    #[test]
    fn a_mention_flag_without_a_count_counts_one() {
        let muted = Conversation {
            is_muted: true,
            mention_me: true,
            ..conversation(4)
        };
        assert_eq!(unread_counted_in_total(&muted), 1);
    }

    #[test]
    fn archived_conversations_count_nothing() {
        let archived = Conversation {
            is_archived: true,
            ..conversation(6)
        };
        assert_eq!(total_unread(&[archived, conversation(1)]), 1);
    }
}
