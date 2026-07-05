//! Receive-side typing presence with TTL, shared by all platforms.
//!
//! Pure state + logic (time injected, like the send-side `typing_should_send`), so it is
//! WASM-safe and unit-testable. The dispatcher feeds inbound per-user `Typing` and server
//! `TypingAggregate` signals in, and reads back the current typing-user set per conversation;
//! entries auto-expire after `ttl` with no refresh so a lost "stop" cannot leave the
//! indicator stuck on. The dispatcher re-emits the aggregated set as `TypingAggregate`,
//! which every platform already consumes via `onTypingAggregateChanged`.

use std::collections::HashMap;

/// Per-conversation typing presence: conversation id -> (user id -> expiry epoch ms).
#[derive(Default)]
pub struct TypingPresence {
    by_conversation: HashMap<String, HashMap<String, u64>>,
}

impl TypingPresence {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a per-user typing signal. `typing=true` inserts/renews the user with
    /// `now + ttl_ms` expiry; `typing=false` removes them immediately. Returns the live
    /// typing-user set for the conversation (expired entries pruned), or `None` when the
    /// set did not change (so callers can skip a redundant re-emit).
    pub fn apply_user(
        &mut self,
        conversation_id: &str,
        user_id: &str,
        typing: bool,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Option<Vec<String>> {
        let before = self.snapshot(conversation_id, now_ms);
        let users = self
            .by_conversation
            .entry(conversation_id.to_string())
            .or_default();
        if typing {
            users.insert(user_id.to_string(), now_ms.saturating_add(ttl_ms));
        } else {
            users.remove(user_id);
        }
        self.finish(conversation_id, now_ms, before)
    }

    /// Apply a server aggregate: the given users are typing now (renewed to `now + ttl`),
    /// anyone previously typing but absent from the list is dropped. Returns the live set
    /// if it changed, else `None`.
    pub fn apply_aggregate(
        &mut self,
        conversation_id: &str,
        typing_user_ids: &[String],
        now_ms: u64,
        ttl_ms: u64,
    ) -> Option<Vec<String>> {
        let before = self.snapshot(conversation_id, now_ms);
        let expiry = now_ms.saturating_add(ttl_ms);
        let users: HashMap<String, u64> = typing_user_ids
            .iter()
            .map(|id| (id.clone(), expiry))
            .collect();
        if users.is_empty() {
            self.by_conversation.remove(conversation_id);
        } else {
            self.by_conversation
                .insert(conversation_id.to_string(), users);
        }
        self.finish(conversation_id, now_ms, before)
    }

    /// Live typing users for a conversation at `now_ms` (expired entries excluded), sorted
    /// for deterministic output.
    pub fn snapshot(&self, conversation_id: &str, now_ms: u64) -> Vec<String> {
        let mut live: Vec<String> = self
            .by_conversation
            .get(conversation_id)
            .into_iter()
            .flat_map(|users| {
                users
                    .iter()
                    .filter(move |&(_, &expiry)| expiry > now_ms)
                    .map(|(id, _)| id.clone())
            })
            .collect();
        live.sort();
        live
    }

    /// Whether any conversation still has (possibly expired) entries — lets the caller stop
    /// its sweep timer once everything is idle.
    pub fn is_empty(&self) -> bool {
        self.by_conversation.is_empty()
    }

    /// Drop expired entries across all conversations. Returns the conversations whose live
    /// set changed, each with its new (possibly empty) set, so the caller can re-emit.
    pub fn prune(&mut self, now_ms: u64) -> Vec<(String, Vec<String>)> {
        let mut changed = Vec::new();
        let conversation_ids: Vec<String> = self.by_conversation.keys().cloned().collect();
        for conversation_id in conversation_ids {
            let mut removed_any = false;
            if let Some(users) = self.by_conversation.get_mut(&conversation_id) {
                let before_len = users.len();
                users.retain(|_, &mut expiry| expiry > now_ms);
                removed_any = users.len() != before_len;
                if users.is_empty() {
                    self.by_conversation.remove(&conversation_id);
                }
            }
            // Report whenever an expired entry is physically dropped, emitting the new live set.
            if removed_any {
                changed.push((
                    conversation_id.clone(),
                    self.snapshot(&conversation_id, now_ms),
                ));
            }
        }
        changed
    }

    fn finish(
        &mut self,
        conversation_id: &str,
        now_ms: u64,
        before: Vec<String>,
    ) -> Option<Vec<String>> {
        if let Some(users) = self.by_conversation.get_mut(conversation_id) {
            if users.is_empty() {
                self.by_conversation.remove(conversation_id);
            }
        }
        let after = self.snapshot(conversation_id, now_ms);
        if before == after { None } else { Some(after) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: u64 = 6_000;

    #[test]
    fn user_typing_then_stop() {
        let mut p = TypingPresence::new();
        assert_eq!(
            p.apply_user("c", "u1", true, 0, TTL),
            Some(vec!["u1".into()])
        );
        // re-applying the same state does not report a change
        assert_eq!(p.apply_user("c", "u1", true, 1000, TTL), None);
        assert_eq!(p.apply_user("c", "u1", false, 2000, TTL), Some(vec![]));
    }

    #[test]
    fn expires_after_ttl_without_refresh() {
        let mut p = TypingPresence::new();
        p.apply_user("c", "u1", true, 0, TTL);
        // still live just before ttl
        assert_eq!(p.snapshot("c", TTL - 1), vec!["u1".to_string()]);
        // prune after ttl clears it and reports the change
        assert_eq!(p.prune(TTL + 1), vec![("c".to_string(), vec![])]);
        assert!(p.is_empty());
    }

    #[test]
    fn refresh_renews_expiry() {
        let mut p = TypingPresence::new();
        p.apply_user("c", "u1", true, 0, TTL);
        p.apply_user("c", "u1", true, TTL - 1000, TTL); // renew before expiry
        // original expiry passed, but renewed keeps it live
        assert_eq!(p.snapshot("c", TTL + 100), vec!["u1".to_string()]);
    }

    #[test]
    fn multiple_users_sorted() {
        let mut p = TypingPresence::new();
        p.apply_user("c", "u2", true, 0, TTL);
        assert_eq!(
            p.apply_user("c", "u1", true, 0, TTL),
            Some(vec!["u1".to_string(), "u2".to_string()])
        );
    }

    #[test]
    fn aggregate_replaces_set() {
        let mut p = TypingPresence::new();
        p.apply_user("c", "u1", true, 0, TTL);
        // server aggregate says u2,u3 are typing now → u1 dropped
        assert_eq!(
            p.apply_aggregate("c", &["u3".into(), "u2".into()], 100, TTL),
            Some(vec!["u2".to_string(), "u3".to_string()])
        );
        // empty aggregate clears
        assert_eq!(p.apply_aggregate("c", &[], 200, TTL), Some(vec![]));
    }
}
