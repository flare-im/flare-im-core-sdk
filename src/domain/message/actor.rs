use crate::model::message::IMMessage;
use crate::shared::error::{ErrorCode, FlareError, Result};

#[derive(Debug, Clone)]
pub struct MessageActor {
    pub user_id: String,
}

impl MessageActor {
    pub fn require(user_id: String) -> Result<Self> {
        if user_id.trim().is_empty() {
            return Err(FlareError::localized(ErrorCode::NotConnected, "未连接"));
        }
        Ok(Self { user_id })
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedMessage {
    pub message: IMMessage,
}

impl ResolvedMessage {
    pub fn new(message: IMMessage) -> Self {
        Self { message }
    }

    pub fn conversation_id(&self) -> &str {
        self.message.conversation_id()
    }

    pub fn sender_id(&self) -> &str {
        self.message.sender_id()
    }

    pub fn server_id(&self) -> &str {
        self.message.server_id()
    }
}
