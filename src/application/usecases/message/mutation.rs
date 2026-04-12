use std::sync::Arc;
use std::time::Duration;

use super::transport_mapper::event_from_transport_action;
use crate::core::CurrentUserIdStore;
use crate::domain::{
    MessageActor, MessageLocalUpdate, MessageLocatorService, MessageMutationService,
    MessageStore, MessageTransportAction, ResolvedMessage,
};
use crate::error::{ErrorCode, FlareError, Result};
use crate::event::{EventBus, MessageEvent, SdkEvent};
use crate::model::message::{MarkType, ReactionAction};
use crate::protocol::PacketSender;

const REQUEST_TIMEOUT_SECS: u64 = 15;
const RESOLVE_WAIT_STEP_MS: u64 = 100;
const RESOLVE_WAIT_TOTAL_MS: u64 = 3_000;

fn timeout() -> Duration {
    Duration::from_secs(REQUEST_TIMEOUT_SECS)
}

pub struct MessageMutationUseCase {
    sender: Arc<PacketSender>,
    store: Arc<dyn MessageStore>,
    current_user_id: CurrentUserIdStore,
    bus: Option<EventBus>,
    locator_service: MessageLocatorService,
    mutation_service: MessageMutationService,
}

impl MessageMutationUseCase {
    pub fn new(
        sender: Arc<PacketSender>,
        store: Arc<dyn MessageStore>,
        current_user_id: CurrentUserIdStore,
        bus: Option<EventBus>,
    ) -> Self {
        Self {
            sender,
            store,
            current_user_id,
            bus,
            locator_service: MessageLocatorService,
            mutation_service: MessageMutationService,
        }
    }

    pub async fn current_user_id(&self) -> Result<String> {
        let uid = self.current_user_id.read().await.clone();
        if uid.is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(uid)
    }

    async fn actor(&self) -> Result<MessageActor> {
        MessageActor::require(self.current_user_id().await?)
    }

    async fn resolve_message(&self, message_id: &str) -> Result<ResolvedMessage> {
        let mut message = self
            .locator_service
            .require_by_any_id(self.store.as_ref(), message_id)
            .await?;
        if message.server_id.is_empty() {
            let mut waited_ms = 0;
            while waited_ms < RESOLVE_WAIT_TOTAL_MS {
                tokio::time::sleep(Duration::from_millis(RESOLVE_WAIT_STEP_MS)).await;
                waited_ms += RESOLVE_WAIT_STEP_MS;
                if let Some(updated) = self
                    .locator_service
                    .find_by_any_id(self.store.as_ref(), message_id)
                    .await?
                {
                    message = updated;
                    if !message.server_id.is_empty() {
                        break;
                    }
                }
            }
        }
        if message.server_id.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::OperationTimeout,
                format!(
                    "message server id not ready yet: {} (try again shortly)",
                    message_id
                ),
            ));
        }
        Ok(ResolvedMessage::new(message))
    }

    pub async fn resolve_message_id(&self, message_id: &str) -> Result<(String, String)> {
        let resolved = self.resolve_message(message_id).await?;
        Ok((
            resolved.conversation_id().to_string(),
            resolved.server_id().to_string(),
        ))
    }

    pub async fn recall(&self, message_id: &str) -> Result<()> {
        let resolved = self.resolve_message(message_id).await?;
        let plan = self.mutation_service.plan_recall(&resolved);
        self.dispatch_transport_action(&plan.transport_action).await?;
        self.store
            .update_status(
                resolved.server_id(),
                flare_proto::common::MessageStatus::Recalled as i32,
            )
            .await?;
        Ok(())
    }

    pub async fn edit(
        &self,
        conversation_id: &str,
        message_id: &str,
        new_content: Vec<u8>,
    ) -> Result<()> {
        let resolved = self.resolve_message(message_id).await?;
        let plan = self
            .mutation_service
            .plan_edit(conversation_id, &resolved, new_content.clone());
        self.dispatch_transport_action(&plan.transport_action).await?;
        self.apply_local_update(plan.local_update).await?;
        if let Some(bus) = &self.bus {
            bus.publish(SdkEvent::Message(MessageEvent::Edited {
                conversation_id: conversation_id.to_string(),
                server_msg_id: resolved.server_id().to_string(),
                edit_version: None,
            }));
        }
        Ok(())
    }

    pub async fn delete_for_self(&self, message_id: &str, reason: Option<String>) -> Result<()> {
        let actor = self.actor().await?;
        let resolved = self.resolve_message(message_id).await?;
        let plan = self
            .mutation_service
            .plan_delete_for_self(&actor, &resolved, reason);
        self.dispatch_transport_action(&plan.transport_action).await?;
        self.apply_local_update(plan.local_update).await
    }

    pub async fn delete_for_everyone(
        &self,
        message_id: &str,
        reason: Option<String>,
    ) -> Result<()> {
        let actor = self.actor().await?;
        let resolved = self.resolve_message(message_id).await?;
        let plan = self
            .mutation_service
            .plan_delete_for_everyone(&actor, &resolved, reason)?;
        self.dispatch_transport_action(&plan.transport_action).await?;
        self.apply_local_update(plan.local_update).await
    }

    pub async fn mark_read_with_ids(
        &self,
        conversation_id: &str,
        message_ids: Vec<String>,
        read_seq: u64,
    ) -> Result<()> {
        let actor = self.actor().await?;
        let plan = self
            .mutation_service
            .plan_read_receipt(&actor, conversation_id, message_ids, read_seq);
        self.dispatch_transport_action(&plan.transport_action).await
    }

    pub async fn typing(&self, conversation_id: &str, typing: bool) -> Result<()> {
        let actor = self.actor().await?;
        let plan = self
            .mutation_service
            .plan_typing(&actor, conversation_id, typing);
        self.dispatch_transport_action(&plan.transport_action).await
    }

    pub async fn add_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        self.react(message_id, emoji, ReactionAction::Add).await
    }

    pub async fn remove_reaction(&self, message_id: &str, emoji: &str) -> Result<()> {
        self.react(message_id, emoji, ReactionAction::Remove).await
    }

    async fn react(&self, message_id: &str, emoji: &str, action: ReactionAction) -> Result<()> {
        let actor = self.actor().await?;
        let resolved = self.resolve_message(message_id).await?;
        let plan = self.mutation_service.plan_reaction(
            &actor,
            resolved.conversation_id(),
            resolved.server_id(),
            emoji,
            action,
        );
        self.dispatch_transport_action(&plan.transport_action).await?;
        self.store
            .apply_reaction(
                resolved.conversation_id(),
                resolved.server_id(),
                &actor.user_id,
                emoji,
                action as i32,
            )
            .await?;
        if let Some(bus) = &self.bus {
            bus.publish(SdkEvent::Message(MessageEvent::ReactionChanged {
                conversation_id: resolved.conversation_id().to_string(),
                server_msg_id: resolved.server_id().to_string(),
                user_id: actor.user_id.clone(),
                emoji: emoji.to_string(),
                action: action as i32,
            }));
        }
        Ok(())
    }

    pub async fn pin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        let actor = self.actor().await?;
        let resolved = self.resolve_message(message_id).await?;
        let plan = self
            .mutation_service
            .plan_pin(&actor, conversation_id, resolved.server_id());
        self.dispatch_transport_action(&plan.transport_action).await
    }

    pub async fn unpin(&self, conversation_id: &str, message_id: &str) -> Result<()> {
        let resolved = self.resolve_message(message_id).await?;
        let plan = self
            .mutation_service
            .plan_unpin(conversation_id, resolved.server_id());
        self.dispatch_transport_action(&plan.transport_action).await
    }

    pub async fn mark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
        color: &str,
    ) -> Result<()> {
        let actor = self.actor().await?;
        let resolved = self.resolve_message(message_id).await?;
        let plan = self.mutation_service.plan_mark(
            &actor,
            conversation_id,
            resolved.server_id(),
            mark_type,
            color,
        );
        self.dispatch_transport_action(&plan.transport_action).await
    }

    pub async fn unmark(
        &self,
        conversation_id: &str,
        message_id: &str,
        mark_type: MarkType,
    ) -> Result<()> {
        let actor = self.actor().await?;
        let resolved = self.resolve_message(message_id).await?;
        let plan = self.mutation_service.plan_unmark(
            &actor,
            conversation_id,
            resolved.server_id(),
            mark_type,
        );
        self.dispatch_transport_action(&plan.transport_action).await
    }

    async fn apply_local_update(&self, update: MessageLocalUpdate) -> Result<()> {
        match update {
            MessageLocalUpdate::None => Ok(()),
            MessageLocalUpdate::UpdateContent {
                message_id,
                new_content,
            } => {
                self.store.update_content(&message_id, new_content).await?;
                Ok(())
            }
            MessageLocalUpdate::Delete { message_id } => self.store.delete(&message_id).await,
        }
    }

    async fn dispatch_transport_action(&self, action: &MessageTransportAction) -> Result<()> {
        self.sender
            .send_event(&event_from_transport_action(action), timeout())
            .await
    }
}
