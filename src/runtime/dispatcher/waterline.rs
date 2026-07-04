use super::*;

impl Dispatcher {
    pub(super) async fn should_apply_delete_for_current_user(
        &self,
        delete: &MessageDeleteEvent,
    ) -> bool {
        let scope = delete.scope.unwrap_or(1);
        if scope != 1 {
            return true;
        }

        let target_user_id = delete.target_user_id.as_deref().unwrap_or_default();
        if target_user_id.is_empty() {
            return true;
        }

        let current = self.current_user_id.read().await.clone();
        !current.is_empty() && current == target_user_id
    }

    pub(super) async fn maybe_trigger_waterline_pull(
        &self,
        source: &str,
        kind: &str,
        conversation_id: Option<&str>,
        attributes: &HashMap<String, String>,
    ) {
        if !is_waterline_ping(kind, attributes) {
            return;
        }
        let conversation_id = conversation_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .or_else(|| attr_non_empty(attributes, WATERLINE_ATTR_CONVERSATION_ID))
            .or_else(|| attr_non_empty(attributes, WATERLINE_ATTR_CONVERSATION_ID_CAMEL));
        let Some(conversation_id) = conversation_id else {
            warn!(source, kind, "waterline ping missing conversation_id");
            return;
        };
        let target_seq = attr_u64(attributes, WATERLINE_ATTR_MAX_SEQ)
            .or_else(|| attr_u64(attributes, WATERLINE_ATTR_MAX_SEQ_CAMEL))
            .unwrap_or(0);
        let user_id = self.current_user_id.read().await.clone();
        if local_waterline_reached_with_stores(&self.stores, &user_id, conversation_id, target_seq)
            .await
        {
            tracing::debug!(
                source,
                kind,
                conversation_id,
                target_seq,
                "waterline already reached locally, skip single conversation pull"
            );
            return;
        }
        let Some(sync) = self.session_sync.clone() else {
            warn!(
                source,
                kind,
                conversation_id,
                target_seq,
                "waterline ping received but SDK has no single conversation sync runner"
            );
            return;
        };

        let mut states = self.waterline_pull_state.lock().await;
        let state = states.entry(conversation_id.to_string()).or_default();
        if state.in_flight {
            state.pending = true;
            state.pending_target_seq = state.pending_target_seq.max(target_seq);
            tracing::debug!(
                source,
                kind,
                conversation_id,
                target_seq,
                "waterline pull already in flight, coalescing ping"
            );
            return;
        }
        state.in_flight = true;
        state.pending = false;
        state.pending_target_seq = 0;
        drop(states);

        let states = self.waterline_pull_state.clone();
        let stores = self.stores.clone();
        let conversation_id = conversation_id.to_string();
        let user_id = user_id.to_string();
        let source = source.to_string();
        let kind = kind.to_string();
        spawn_background(async move {
            let mut active_target_seq = target_seq;
            loop {
                let result = sync.request_message_sync(&conversation_id).await;
                if let Err(error) = &result {
                    warn!(
                        source = %source,
                        kind = %kind,
                        conversation_id = %conversation_id,
                        target_seq = active_target_seq,
                        error = %error,
                        "waterline-triggered single conversation pull failed"
                    );
                }

                let pending_target_seq = {
                    let mut states = states.lock().await;
                    let Some(state) = states.get_mut(&conversation_id) else {
                        break;
                    };
                    if !state.pending {
                        states.remove(&conversation_id);
                        break;
                    }
                    state.pending = false;
                    let pending_target_seq = state.pending_target_seq;
                    state.pending_target_seq = 0;
                    pending_target_seq
                };

                if local_waterline_reached_with_stores(
                    &stores,
                    &user_id,
                    &conversation_id,
                    pending_target_seq,
                )
                .await
                {
                    let mut states = states.lock().await;
                    if let Some(state) = states.get(&conversation_id)
                        && !state.pending
                    {
                        states.remove(&conversation_id);
                        break;
                    }
                }
                active_target_seq = pending_target_seq;
            }
        });
    }

    pub(super) async fn maybe_trigger_event_envelope_waterline(
        &self,
        envelope: &flare_proto::common::EventEnvelope,
    ) {
        let conversation_id = envelope.conversation_id.trim();
        let target_seq = envelope.max_conversation_seq;
        if conversation_id.is_empty() || target_seq == 0 {
            return;
        }
        let attributes =
            HashMap::from([(WATERLINE_ATTR_MAX_SEQ.to_string(), target_seq.to_string())]);
        self.maybe_trigger_waterline_pull(
            "event_envelope",
            "conversation.waterline",
            Some(conversation_id),
            &attributes,
        )
        .await;
    }

    pub(super) async fn maybe_trigger_event_waterline(&self, event: &flare_proto::common::Event) {
        let conversation_id = Self::event_conversation_id(event);
        let target_seq = event.conversation_seq;
        if conversation_id.is_empty() || target_seq == 0 {
            return;
        }
        let attributes =
            HashMap::from([(WATERLINE_ATTR_MAX_SEQ.to_string(), target_seq.to_string())]);
        self.maybe_trigger_waterline_pull(
            "event",
            "conversation.waterline",
            Some(conversation_id),
            &attributes,
        )
        .await;
    }

    pub(super) async fn local_waterline_reached(
        &self,
        conversation_id: &str,
        target_seq: u64,
    ) -> bool {
        let user_id = self.current_user_id.read().await.clone();
        local_waterline_reached_with_stores(&self.stores, &user_id, conversation_id, target_seq)
            .await
    }
}
