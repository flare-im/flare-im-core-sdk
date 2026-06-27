//! Conversation encryption policy and E2EE content wrapping primitives.
//!
//! This module intentionally does not implement a concrete ratchet, MLS, or
//! sender-key protocol. Those algorithms belong in business/security plugins
//! that provide a [`ContentCodec`]. Core owns the stable tier contract and the
//! fail-closed message pipeline that turns plaintext `MessageContent` into a
//! typed encrypted placeholder before transport.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_proto::common::message_content::Content as ProtoContent;
use flare_proto::common::{MessageContent, MessageType, PlaceholderContent};
use prost::Message as ProstMessage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::extension::middleware::{
    MessageInterceptor, MessageMiddlewareContext, MessageOperation,
};
use crate::extension::{ContentCodec, ExtensionContent};
use crate::model::IMMessage;
use crate::shared::error::{ErrorCode, FlareError, Result};

pub const E2EE_PLACEHOLDER_REASON: &str = "e2e_ciphertext";
pub const E2EE_FALLBACK_TEXT: &str = "[Encrypted message]";
pub const PLAINTEXT_CONTENT_TYPE: &str = "application/vnd.flare.message-content.v1";
pub const E2EE_CONTENT_TYPE: &str = "application/vnd.flare.e2ee-message.v1";

pub const E2EE_ATTR_TIER: &str = "tier";
pub const E2EE_ATTR_CODEC_NAMESPACE: &str = "codecNamespace";
pub const E2EE_ATTR_CODEC_CONTENT_TYPE: &str = "codecContentType";
pub const E2EE_ATTR_SUITE: &str = "suite";
pub const E2EE_ATTR_KEY_ID: &str = "keyId";
pub const E2EE_ATTR_SENDER_KEY_ID: &str = "senderKeyId";
pub const E2EE_ATTR_DEVICE_SESSION_ID: &str = "deviceSessionId";
pub const E2EE_ATTR_PLAINTEXT_MESSAGE_TYPE: &str = "plaintextMessageType";
pub const E2EE_ATTR_OPERATION: &str = "operation";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct E2eeIdentityKey {
    pub user_id: String,
    pub device_id: String,
    pub key_id: String,
    pub public_key: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct E2eePreKeyBundle {
    pub user_id: String,
    pub device_id: String,
    pub identity_key_id: String,
    pub signed_pre_key_id: String,
    pub signed_pre_key: Vec<u8>,
    pub signature: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_time_pre_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_time_pre_key: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct E2eeSessionDescriptor {
    pub conversation_id: String,
    pub device_session_id: String,
    pub local_device_id: String,
    pub peer_device_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait E2eeKeyManager: Send + Sync {
    async fn local_identity_key(&self) -> Result<E2eeIdentityKey>;

    async fn publish_pre_key_bundle(&self, bundle: E2eePreKeyBundle) -> Result<()>;

    async fn establish_session(
        &self,
        conversation_id: &str,
        peer_bundles: Vec<E2eePreKeyBundle>,
    ) -> Result<E2eeSessionDescriptor>;

    async fn session_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<E2eeSessionDescriptor>>;

    async fn revoke_device_session(&self, device_session_id: &str) -> Result<()>;
}

#[derive(Default)]
pub struct VolatileE2eeKeyManager {
    identity_key: RwLock<Option<E2eeIdentityKey>>,
    pre_key_bundles: RwLock<HashMap<String, E2eePreKeyBundle>>,
    sessions: RwLock<HashMap<String, E2eeSessionDescriptor>>,
}

impl VolatileE2eeKeyManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn set_local_identity_key(&self, identity_key: E2eeIdentityKey) -> Result<()> {
        validate_identity_key(&identity_key)?;
        *self.identity_key.write().await = Some(identity_key);
        Ok(())
    }

    pub async fn upsert_session(&self, session: E2eeSessionDescriptor) -> Result<()> {
        let conversation_id = validate_session_descriptor(&session)?;
        self.sessions.write().await.insert(conversation_id, session);
        Ok(())
    }

    pub async fn pre_key_bundle(
        &self,
        user_id: &str,
        device_id: &str,
        signed_pre_key_id: &str,
    ) -> Result<Option<E2eePreKeyBundle>> {
        Ok(self
            .pre_key_bundles
            .read()
            .await
            .get(&pre_key_bundle_key(user_id, device_id, signed_pre_key_id)?)
            .cloned())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl E2eeKeyManager for VolatileE2eeKeyManager {
    async fn local_identity_key(&self) -> Result<E2eeIdentityKey> {
        self.identity_key
            .read()
            .await
            .clone()
            .ok_or_else(|| e2ee_invalid("E2EE local identity key is not configured"))
    }

    async fn publish_pre_key_bundle(&self, bundle: E2eePreKeyBundle) -> Result<()> {
        let key = validate_pre_key_bundle(&bundle)?;
        self.pre_key_bundles.write().await.insert(key, bundle);
        Ok(())
    }

    async fn establish_session(
        &self,
        conversation_id: &str,
        _peer_bundles: Vec<E2eePreKeyBundle>,
    ) -> Result<E2eeSessionDescriptor> {
        let conversation_id =
            required_policy_value("E2EE conversation_id", conversation_id.to_string())?;
        if let Some(session) = self.session_for_conversation(&conversation_id).await? {
            return Ok(session);
        }
        Err(FlareError::localized(
            ErrorCode::ConfigurationError,
            "E2EE session establishment requires a concrete crypto provider",
        ))
    }

    async fn session_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Option<E2eeSessionDescriptor>> {
        let conversation_id =
            required_policy_value("E2EE conversation_id", conversation_id.to_string())?;
        Ok(self.sessions.read().await.get(&conversation_id).cloned())
    }

    async fn revoke_device_session(&self, device_session_id: &str) -> Result<()> {
        let device_session_id =
            required_policy_value("E2EE device_session_id", device_session_id.to_string())?;
        self.sessions
            .write()
            .await
            .retain(|_, session| session.device_session_id != device_session_id);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionTier {
    /// No SDK-level content protection.
    None,
    /// Transport security only. Content remains normal `MessageContent`.
    #[default]
    Transport,
    /// End-to-end encrypted content envelope. Requires a configured
    /// [`ContentCodec`] and fails closed when the codec cannot produce
    /// ciphertext.
    E2e,
}

impl EncryptionTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Transport => "transport",
            Self::E2e => "e2e",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEncryptionPolicy {
    pub tier: EncryptionTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_session_id: Option<String>,
}

impl ConversationEncryptionPolicy {
    pub fn none() -> Self {
        Self::new(EncryptionTier::None)
    }

    pub fn transport() -> Self {
        Self::new(EncryptionTier::Transport)
    }

    pub fn e2e() -> Self {
        Self::new(EncryptionTier::E2e)
    }

    pub fn new(tier: EncryptionTier) -> Self {
        Self {
            tier,
            suite: None,
            codec_namespace: None,
            key_id: None,
            sender_key_id: None,
            device_session_id: None,
        }
    }

    pub fn with_suite(mut self, suite: impl Into<String>) -> Self {
        self.suite = non_empty(suite.into());
        self
    }

    pub fn with_codec_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.codec_namespace = non_empty(namespace.into());
        self
    }

    pub fn with_key_id(mut self, key_id: impl Into<String>) -> Self {
        self.key_id = non_empty(key_id.into());
        self
    }

    pub fn with_sender_key_id(mut self, sender_key_id: impl Into<String>) -> Self {
        self.sender_key_id = non_empty(sender_key_id.into());
        self
    }

    pub fn with_device_session_id(mut self, device_session_id: impl Into<String>) -> Self {
        self.device_session_id = non_empty(device_session_id.into());
        self
    }
}

impl Default for ConversationEncryptionPolicy {
    fn default() -> Self {
        Self::transport()
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ConversationEncryptionPolicyResolver: Send + Sync {
    async fn policy_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationEncryptionPolicy>;

    async fn policy_for_message(
        &self,
        message: &IMMessage,
    ) -> Result<ConversationEncryptionPolicy> {
        self.policy_for_conversation(&message.conversation_id).await
    }
}

#[derive(Debug, Clone)]
pub struct StaticConversationEncryptionPolicyResolver {
    default_policy: ConversationEncryptionPolicy,
    conversation_policies: HashMap<String, ConversationEncryptionPolicy>,
}

impl StaticConversationEncryptionPolicyResolver {
    pub fn new(default_policy: ConversationEncryptionPolicy) -> Self {
        Self {
            default_policy,
            conversation_policies: HashMap::new(),
        }
    }

    pub fn transport() -> Self {
        Self::new(ConversationEncryptionPolicy::transport())
    }

    pub fn with_conversation_policy(
        mut self,
        conversation_id: impl Into<String>,
        policy: ConversationEncryptionPolicy,
    ) -> Self {
        if let Some(conversation_id) = non_empty(conversation_id.into()) {
            self.conversation_policies.insert(conversation_id, policy);
        }
        self
    }
}

impl Default for StaticConversationEncryptionPolicyResolver {
    fn default() -> Self {
        Self::transport()
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ConversationEncryptionPolicyResolver for StaticConversationEncryptionPolicyResolver {
    async fn policy_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationEncryptionPolicy> {
        Ok(self
            .conversation_policies
            .get(conversation_id)
            .cloned()
            .unwrap_or_else(|| self.default_policy.clone()))
    }
}

#[derive(Clone)]
pub struct KeyManagedConversationEncryptionPolicyResolver {
    key_manager: Arc<dyn E2eeKeyManager>,
    codec_namespace: Option<String>,
    default_suite: Option<String>,
}

impl KeyManagedConversationEncryptionPolicyResolver {
    pub fn new(key_manager: Arc<dyn E2eeKeyManager>) -> Self {
        Self {
            key_manager,
            codec_namespace: None,
            default_suite: None,
        }
    }

    pub fn with_codec_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.codec_namespace = non_empty(namespace.into());
        self
    }

    pub fn with_default_suite(mut self, suite: impl Into<String>) -> Self {
        self.default_suite = non_empty(suite.into());
        self
    }

    fn policy_from_session(
        &self,
        requested_conversation_id: &str,
        session: E2eeSessionDescriptor,
    ) -> Result<ConversationEncryptionPolicy> {
        let session_conversation_id =
            required_policy_value("E2EE session conversation_id", session.conversation_id)?;
        if session_conversation_id != requested_conversation_id {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "E2EE session conversation_id mismatch",
            ));
        }
        let device_session_id =
            required_policy_value("E2EE device_session_id", session.device_session_id)?;
        let key_id = non_empty_opt(session.key_id);
        let sender_key_id = non_empty_opt(session.sender_key_id);
        if key_id.is_none() && sender_key_id.is_none() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "E2EE session requires key_id or sender_key_id",
            ));
        }
        let mut policy =
            ConversationEncryptionPolicy::e2e().with_device_session_id(device_session_id);
        if let Some(codec_namespace) = self.codec_namespace.as_deref() {
            policy = policy.with_codec_namespace(codec_namespace);
        }
        if let Some(suite) = non_empty_opt(session.suite).or_else(|| self.default_suite.clone()) {
            policy = policy.with_suite(suite);
        }
        if let Some(key_id) = key_id {
            policy = policy.with_key_id(key_id);
        }
        if let Some(sender_key_id) = sender_key_id {
            policy = policy.with_sender_key_id(sender_key_id);
        }
        Ok(policy)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl ConversationEncryptionPolicyResolver for KeyManagedConversationEncryptionPolicyResolver {
    async fn policy_for_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<ConversationEncryptionPolicy> {
        let conversation_id =
            required_policy_value("E2EE conversation_id", conversation_id.to_string())?;
        let Some(session) = self
            .key_manager
            .session_for_conversation(&conversation_id)
            .await?
        else {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "E2EE session is not established for conversation",
            ));
        };
        self.policy_from_session(&conversation_id, session)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedContentEnvelope {
    pub ciphertext: Vec<u8>,
    pub attributes: HashMap<String, String>,
}

impl EncryptedContentEnvelope {
    pub fn codec_namespace(&self) -> Option<&str> {
        self.attributes
            .get(E2EE_ATTR_CODEC_NAMESPACE)
            .map(String::as_str)
    }

    pub fn suite(&self) -> Option<&str> {
        self.attributes.get(E2EE_ATTR_SUITE).map(String::as_str)
    }

    pub fn key_id(&self) -> Option<&str> {
        self.attributes.get(E2EE_ATTR_KEY_ID).map(String::as_str)
    }
}

pub struct ContentEncryptionInterceptor {
    resolver: Arc<dyn ConversationEncryptionPolicyResolver>,
    codec: Arc<dyn ContentCodec>,
}

impl ContentEncryptionInterceptor {
    pub fn new(
        resolver: Arc<dyn ConversationEncryptionPolicyResolver>,
        codec: Arc<dyn ContentCodec>,
    ) -> Self {
        Self { resolver, codec }
    }

    fn validate_codec(&self, policy: &ConversationEncryptionPolicy) -> Result<()> {
        if let Some(expected) = policy.codec_namespace.as_deref()
            && expected != self.codec.namespace()
        {
            return Err(FlareError::localized(
                ErrorCode::ConfigurationError,
                format!(
                    "E2EE codec namespace mismatch: expected {}, got {}",
                    expected,
                    self.codec.namespace()
                ),
            ));
        }
        Ok(())
    }

    fn encrypt_message(
        &self,
        message: &mut IMMessage,
        policy: &ConversationEncryptionPolicy,
        ctx: &MessageMiddlewareContext,
    ) -> Result<()> {
        if encrypted_content_envelope(message).is_some() {
            return Ok(());
        }
        self.validate_codec(policy)?;
        message.materialize_encoded_content_from_elem();
        if message.encoded_content.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "E2EE requires non-empty MessageContent plaintext",
            ));
        }

        let plaintext_message_type = message.message_type.to_string();
        let mut attributes = envelope_attributes(policy, self.codec.as_ref(), ctx);
        attributes.insert(
            E2EE_ATTR_PLAINTEXT_MESSAGE_TYPE.to_string(),
            plaintext_message_type,
        );
        let plaintext = ExtensionContent {
            content_type: PLAINTEXT_CONTENT_TYPE.to_string(),
            payload: message.encoded_content.clone(),
            attributes: attributes.clone(),
        };
        let ciphertext = self.codec.encode(&plaintext)?;
        if ciphertext.is_empty() {
            return Err(FlareError::localized(
                ErrorCode::InvalidParameter,
                "E2EE content codec returned empty ciphertext",
            ));
        }

        let encrypted_content = MessageContent {
            content: Some(ProtoContent::Placeholder(PlaceholderContent {
                reason: E2EE_PLACEHOLDER_REASON.to_string(),
                payload: ciphertext,
                fallback_text: E2EE_FALLBACK_TEXT.to_string(),
                attributes,
            })),
        };
        message.message_type = MessageType::Placeholder as i32;
        message.content = None;
        message.encoded_content = encrypted_content.encode_to_vec();
        message.text_preview = E2EE_FALLBACK_TEXT.to_string();
        Ok(())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl MessageInterceptor for ContentEncryptionInterceptor {
    async fn before_send(
        &self,
        message: &mut IMMessage,
        ctx: &MessageMiddlewareContext,
    ) -> Result<()> {
        let policy = self.resolver.policy_for_message(message).await?;
        match policy.tier {
            EncryptionTier::None | EncryptionTier::Transport => Ok(()),
            EncryptionTier::E2e => self.encrypt_message(message, &policy, ctx),
        }
    }
}

pub fn encrypted_content_envelope(message: &IMMessage) -> Option<EncryptedContentEnvelope> {
    encrypted_content_envelope_from_bytes(&message.encoded_content)
}

pub fn encrypted_content_envelope_from_bytes(bytes: &[u8]) -> Option<EncryptedContentEnvelope> {
    let content = MessageContent::decode(bytes).ok()?;
    let Some(ProtoContent::Placeholder(placeholder)) = content.content else {
        return None;
    };
    if placeholder.reason != E2EE_PLACEHOLDER_REASON {
        return None;
    }
    Some(EncryptedContentEnvelope {
        ciphertext: placeholder.payload,
        attributes: placeholder.attributes,
    })
}

fn envelope_attributes(
    policy: &ConversationEncryptionPolicy,
    codec: &dyn ContentCodec,
    ctx: &MessageMiddlewareContext,
) -> HashMap<String, String> {
    let mut attributes = HashMap::new();
    attributes.insert(
        E2EE_ATTR_TIER.to_string(),
        EncryptionTier::E2e.as_str().to_string(),
    );
    attributes.insert(
        E2EE_ATTR_CODEC_NAMESPACE.to_string(),
        codec.namespace().to_string(),
    );
    attributes.insert(
        E2EE_ATTR_CODEC_CONTENT_TYPE.to_string(),
        non_empty(codec.content_type().to_string())
            .unwrap_or_else(|| E2EE_CONTENT_TYPE.to_string()),
    );
    attributes.insert(
        E2EE_ATTR_OPERATION.to_string(),
        match ctx.operation {
            MessageOperation::DirectSend => "direct_send",
            MessageOperation::ReliableQueueEnqueue => "reliable_queue_enqueue",
        }
        .to_string(),
    );
    insert_optional(&mut attributes, E2EE_ATTR_SUITE, &policy.suite);
    insert_optional(&mut attributes, E2EE_ATTR_KEY_ID, &policy.key_id);
    insert_optional(
        &mut attributes,
        E2EE_ATTR_SENDER_KEY_ID,
        &policy.sender_key_id,
    );
    insert_optional(
        &mut attributes,
        E2EE_ATTR_DEVICE_SESSION_ID,
        &policy.device_session_id,
    );
    attributes
}

fn insert_optional(map: &mut HashMap<String, String>, key: &str, value: &Option<String>) {
    if let Some(value) = value.as_deref().and_then(|v| non_empty(v.to_string())) {
        map.insert(key.to_string(), value);
    }
}

fn required_policy_value(field: &'static str, value: String) -> Result<String> {
    non_empty(value).ok_or_else(|| {
        FlareError::localized(
            ErrorCode::InvalidParameter,
            format!("{field} must not be empty"),
        )
    })
}

fn validate_identity_key(identity_key: &E2eeIdentityKey) -> Result<()> {
    required_policy_value("E2EE identity user_id", identity_key.user_id.clone())?;
    required_policy_value("E2EE identity device_id", identity_key.device_id.clone())?;
    required_policy_value("E2EE identity key_id", identity_key.key_id.clone())?;
    if identity_key.public_key.is_empty() {
        return Err(e2ee_invalid("E2EE identity public_key must not be empty"));
    }
    Ok(())
}

fn validate_pre_key_bundle(bundle: &E2eePreKeyBundle) -> Result<String> {
    let user_id = required_policy_value("E2EE pre-key user_id", bundle.user_id.clone())?;
    let device_id = required_policy_value("E2EE pre-key device_id", bundle.device_id.clone())?;
    required_policy_value(
        "E2EE pre-key identity_key_id",
        bundle.identity_key_id.clone(),
    )?;
    let signed_pre_key_id =
        required_policy_value("E2EE signed_pre_key_id", bundle.signed_pre_key_id.clone())?;
    if bundle.signed_pre_key.is_empty() {
        return Err(e2ee_invalid("E2EE signed_pre_key must not be empty"));
    }
    if bundle.signature.is_empty() {
        return Err(e2ee_invalid("E2EE pre-key signature must not be empty"));
    }
    match (&bundle.one_time_pre_key_id, &bundle.one_time_pre_key) {
        (Some(id), Some(key)) => {
            required_policy_value("E2EE one_time_pre_key_id", id.clone())?;
            if key.is_empty() {
                return Err(e2ee_invalid("E2EE one_time_pre_key must not be empty"));
            }
        }
        (None, None) => {}
        _ => {
            return Err(e2ee_invalid(
                "E2EE one-time pre-key id and key must be provided together",
            ));
        }
    }
    pre_key_bundle_key(&user_id, &device_id, &signed_pre_key_id)
}

fn validate_session_descriptor(session: &E2eeSessionDescriptor) -> Result<String> {
    let conversation_id = required_policy_value(
        "E2EE session conversation_id",
        session.conversation_id.clone(),
    )?;
    required_policy_value("E2EE device_session_id", session.device_session_id.clone())?;
    required_policy_value(
        "E2EE session local_device_id",
        session.local_device_id.clone(),
    )?;
    if session.peer_device_ids.is_empty() {
        return Err(e2ee_invalid(
            "E2EE session peer_device_ids must not be empty",
        ));
    }
    for peer_device_id in &session.peer_device_ids {
        required_policy_value("E2EE peer_device_id", peer_device_id.clone())?;
    }
    if non_empty_opt(session.key_id.clone()).is_none()
        && non_empty_opt(session.sender_key_id.clone()).is_none()
    {
        return Err(e2ee_invalid(
            "E2EE session requires key_id or sender_key_id",
        ));
    }
    Ok(conversation_id)
}

fn pre_key_bundle_key(user_id: &str, device_id: &str, signed_pre_key_id: &str) -> Result<String> {
    let user_id = required_policy_value("E2EE pre-key user_id", user_id.to_string())?;
    let device_id = required_policy_value("E2EE pre-key device_id", device_id.to_string())?;
    let signed_pre_key_id =
        required_policy_value("E2EE signed_pre_key_id", signed_pre_key_id.to_string())?;
    Ok(format!(
        "{user_id}\u{1f}{device_id}\u{1f}{signed_pre_key_id}"
    ))
}

fn e2ee_invalid(message: impl Into<String>) -> FlareError {
    FlareError::localized(ErrorCode::InvalidParameter, message.into())
}

fn non_empty_opt(value: Option<String>) -> Option<String> {
    value.and_then(non_empty)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else if trimmed.len() == value.len() {
        Some(value)
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::MessageBuilder;
    use flare_proto::common::Message as ProtoMessage;

    struct StaticKeyManager {
        session: Option<E2eeSessionDescriptor>,
    }

    #[async_trait]
    impl E2eeKeyManager for StaticKeyManager {
        async fn local_identity_key(&self) -> Result<E2eeIdentityKey> {
            Ok(E2eeIdentityKey {
                user_id: "alice".to_string(),
                device_id: "device-a".to_string(),
                key_id: "identity-key-1".to_string(),
                public_key: vec![1, 2, 3],
                suite: Some("test-suite".to_string()),
            })
        }

        async fn publish_pre_key_bundle(&self, _bundle: E2eePreKeyBundle) -> Result<()> {
            Ok(())
        }

        async fn establish_session(
            &self,
            conversation_id: &str,
            _peer_bundles: Vec<E2eePreKeyBundle>,
        ) -> Result<E2eeSessionDescriptor> {
            Ok(self
                .session
                .clone()
                .unwrap_or_else(|| E2eeSessionDescriptor {
                    conversation_id: conversation_id.to_string(),
                    device_session_id: "device-session-1".to_string(),
                    local_device_id: "device-a".to_string(),
                    peer_device_ids: vec!["device-b".to_string()],
                    key_id: Some("key-1".to_string()),
                    sender_key_id: None,
                    suite: Some("test-suite".to_string()),
                }))
        }

        async fn session_for_conversation(
            &self,
            _conversation_id: &str,
        ) -> Result<Option<E2eeSessionDescriptor>> {
            Ok(self.session.clone())
        }

        async fn revoke_device_session(&self, _device_session_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct PrefixCodec;

    impl ContentCodec for PrefixCodec {
        fn namespace(&self) -> &str {
            "test.e2ee"
        }

        fn content_type(&self) -> &str {
            E2EE_CONTENT_TYPE
        }

        fn encode(&self, content: &ExtensionContent) -> Result<Vec<u8>> {
            assert_eq!(content.content_type, PLAINTEXT_CONTENT_TYPE);
            let mut out = b"cipher:".to_vec();
            out.extend_from_slice(&content.payload);
            Ok(out)
        }

        fn decode(&self, payload: &[u8]) -> Result<ExtensionContent> {
            let plaintext = payload.strip_prefix(b"cipher:").unwrap_or(payload).to_vec();
            Ok(ExtensionContent {
                content_type: PLAINTEXT_CONTENT_TYPE.to_string(),
                payload: plaintext,
                attributes: HashMap::new(),
            })
        }
    }

    fn text_message() -> IMMessage {
        MessageBuilder::text("conv-1", "alice", "hello")
            .expect("text message")
            .into()
    }

    fn session() -> E2eeSessionDescriptor {
        E2eeSessionDescriptor {
            conversation_id: "conv-1".to_string(),
            device_session_id: "device-session-1".to_string(),
            local_device_id: "device-a".to_string(),
            peer_device_ids: vec!["device-b".to_string()],
            key_id: Some("key-1".to_string()),
            sender_key_id: Some("sender-key-1".to_string()),
            suite: Some("test-suite".to_string()),
        }
    }

    fn identity_key() -> E2eeIdentityKey {
        E2eeIdentityKey {
            user_id: "alice".to_string(),
            device_id: "device-a".to_string(),
            key_id: "identity-key-1".to_string(),
            public_key: vec![1, 2, 3],
            suite: Some("test-suite".to_string()),
        }
    }

    fn pre_key_bundle() -> E2eePreKeyBundle {
        E2eePreKeyBundle {
            user_id: "bob".to_string(),
            device_id: "device-b".to_string(),
            identity_key_id: "identity-key-b".to_string(),
            signed_pre_key_id: "signed-pre-key-1".to_string(),
            signed_pre_key: vec![4, 5, 6],
            signature: vec![7, 8, 9],
            one_time_pre_key_id: Some("one-time-pre-key-1".to_string()),
            one_time_pre_key: Some(vec![10, 11, 12]),
            suite: Some("test-suite".to_string()),
        }
    }

    #[tokio::test]
    async fn volatile_key_manager_stores_identity_prekeys_and_sessions() {
        let manager = VolatileE2eeKeyManager::new();
        let missing_identity = manager
            .local_identity_key()
            .await
            .expect_err("identity must be explicitly configured");
        assert_eq!(missing_identity.code(), Some(ErrorCode::InvalidParameter));

        manager
            .set_local_identity_key(identity_key())
            .await
            .expect("identity key");
        assert_eq!(manager.local_identity_key().await.unwrap(), identity_key());

        manager
            .publish_pre_key_bundle(pre_key_bundle())
            .await
            .expect("pre-key bundle");
        assert_eq!(
            manager
                .pre_key_bundle("bob", "device-b", "signed-pre-key-1")
                .await
                .unwrap(),
            Some(pre_key_bundle())
        );
        assert!(
            manager
                .pre_key_bundle("bob", "device-x", "signed-pre-key-1")
                .await
                .unwrap()
                .is_none()
        );

        manager.upsert_session(session()).await.expect("session");
        assert_eq!(
            manager
                .session_for_conversation("conv-1")
                .await
                .unwrap()
                .map(|session| session.device_session_id),
            Some("device-session-1".to_string())
        );
        assert_eq!(
            manager
                .establish_session("conv-1", Vec::new())
                .await
                .unwrap(),
            session()
        );

        manager
            .revoke_device_session("device-session-1")
            .await
            .expect("revoke");
        assert!(
            manager
                .session_for_conversation("conv-1")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn volatile_key_manager_fails_closed_without_crypto_establishment() {
        let manager = VolatileE2eeKeyManager::new();

        let err = manager
            .establish_session("conv-1", vec![pre_key_bundle()])
            .await
            .expect_err("volatile manager must not fake cryptographic session setup");

        assert_eq!(err.code(), Some(ErrorCode::ConfigurationError));
    }

    #[tokio::test]
    async fn volatile_key_manager_rejects_incomplete_session_descriptors() {
        let manager = VolatileE2eeKeyManager::new();
        let mut invalid_session = session();
        invalid_session.key_id = None;
        invalid_session.sender_key_id = None;

        let err = manager
            .upsert_session(invalid_session)
            .await
            .expect_err("session without key material reference must fail");

        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }

    #[tokio::test]
    async fn key_managed_resolver_uses_volatile_key_manager_session() {
        let manager = Arc::new(VolatileE2eeKeyManager::new());
        manager.upsert_session(session()).await.expect("session");
        let resolver =
            KeyManagedConversationEncryptionPolicyResolver::new(manager).with_default_suite("x3dh");

        let policy = resolver.policy_for_conversation("conv-1").await.unwrap();

        assert_eq!(policy.tier, EncryptionTier::E2e);
        assert_eq!(
            policy.device_session_id.as_deref(),
            Some("device-session-1")
        );
        assert_eq!(policy.key_id.as_deref(), Some("key-1"));
        assert_eq!(policy.sender_key_id.as_deref(), Some("sender-key-1"));
    }

    #[tokio::test]
    async fn key_managed_resolver_builds_e2e_policy_from_session() {
        let resolver =
            KeyManagedConversationEncryptionPolicyResolver::new(Arc::new(StaticKeyManager {
                session: Some(session()),
            }))
            .with_codec_namespace("test.e2ee")
            .with_default_suite("fallback-suite");

        let policy = resolver.policy_for_conversation("conv-1").await.unwrap();

        assert_eq!(policy.tier, EncryptionTier::E2e);
        assert_eq!(policy.codec_namespace.as_deref(), Some("test.e2ee"));
        assert_eq!(policy.suite.as_deref(), Some("test-suite"));
        assert_eq!(policy.key_id.as_deref(), Some("key-1"));
        assert_eq!(policy.sender_key_id.as_deref(), Some("sender-key-1"));
        assert_eq!(
            policy.device_session_id.as_deref(),
            Some("device-session-1")
        );
    }

    #[tokio::test]
    async fn key_managed_resolver_fails_when_session_is_missing() {
        let resolver =
            KeyManagedConversationEncryptionPolicyResolver::new(Arc::new(StaticKeyManager {
                session: None,
            }));

        let err = resolver
            .policy_for_conversation("conv-1")
            .await
            .expect_err("E2EE without an established session must fail");

        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }

    #[tokio::test]
    async fn e2e_policy_wraps_plaintext_content_in_encrypted_placeholder() {
        let resolver = Arc::new(StaticConversationEncryptionPolicyResolver::new(
            ConversationEncryptionPolicy::e2e()
                .with_codec_namespace("test.e2ee")
                .with_suite("test-suite")
                .with_sender_key_id("sender-key-1")
                .with_device_session_id("device-session-1"),
        ));
        let interceptor = ContentEncryptionInterceptor::new(resolver, Arc::new(PrefixCodec));
        let ctx = MessageMiddlewareContext::new(MessageOperation::DirectSend);
        let mut message = text_message();

        interceptor.before_send(&mut message, &ctx).await.unwrap();

        assert_eq!(message.message_type, MessageType::Placeholder as i32);
        assert!(message.content.is_none());
        let envelope = encrypted_content_envelope(&message).expect("encrypted envelope");
        assert!(envelope.ciphertext.starts_with(b"cipher:"));
        assert_eq!(envelope.codec_namespace(), Some("test.e2ee"));
        assert_eq!(envelope.suite(), Some("test-suite"));
        assert_eq!(
            envelope
                .attributes
                .get(E2EE_ATTR_SENDER_KEY_ID)
                .map(String::as_str),
            Some("sender-key-1")
        );
        assert_eq!(
            envelope
                .attributes
                .get(E2EE_ATTR_DEVICE_SESSION_ID)
                .map(String::as_str),
            Some("device-session-1")
        );
    }

    #[tokio::test]
    async fn key_managed_resolver_feeds_session_attributes_into_envelope() {
        let resolver = Arc::new(
            KeyManagedConversationEncryptionPolicyResolver::new(Arc::new(StaticKeyManager {
                session: Some(session()),
            }))
            .with_codec_namespace("test.e2ee"),
        );
        let interceptor = ContentEncryptionInterceptor::new(resolver, Arc::new(PrefixCodec));
        let ctx = MessageMiddlewareContext::new(MessageOperation::DirectSend);
        let mut message = text_message();

        interceptor.before_send(&mut message, &ctx).await.unwrap();

        let envelope = encrypted_content_envelope(&message).expect("encrypted envelope");
        assert_eq!(envelope.key_id(), Some("key-1"));
        assert_eq!(
            envelope
                .attributes
                .get(E2EE_ATTR_DEVICE_SESSION_ID)
                .map(String::as_str),
            Some("device-session-1")
        );
        assert_eq!(
            envelope
                .attributes
                .get(E2EE_ATTR_SENDER_KEY_ID)
                .map(String::as_str),
            Some("sender-key-1")
        );
    }

    #[tokio::test]
    async fn transport_policy_leaves_plaintext_content_unchanged() {
        let resolver = Arc::new(StaticConversationEncryptionPolicyResolver::transport());
        let interceptor = ContentEncryptionInterceptor::new(resolver, Arc::new(PrefixCodec));
        let ctx = MessageMiddlewareContext::new(MessageOperation::DirectSend);
        let mut message = text_message();

        interceptor.before_send(&mut message, &ctx).await.unwrap();

        assert_ne!(message.message_type, MessageType::Placeholder as i32);
        assert!(encrypted_content_envelope(&message).is_none());
    }

    #[tokio::test]
    async fn e2e_policy_rejects_empty_plaintext_content() {
        let resolver = Arc::new(StaticConversationEncryptionPolicyResolver::new(
            ConversationEncryptionPolicy::e2e(),
        ));
        let interceptor = ContentEncryptionInterceptor::new(resolver, Arc::new(PrefixCodec));
        let ctx = MessageMiddlewareContext::new(MessageOperation::DirectSend);
        let mut message = IMMessage::new(ProtoMessage::default());
        message.conversation_id = "conv-1".to_string();

        let err = interceptor
            .before_send(&mut message, &ctx)
            .await
            .expect_err("empty plaintext must fail closed");
        assert_eq!(err.code(), Some(ErrorCode::InvalidParameter));
    }
}
