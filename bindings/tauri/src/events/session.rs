//! 会话事件转发
//!
//! 将 SDK 会话事件（登录、登出、过期等）自动转发到 Tauri 前端

use tauri::{AppHandle, Emitter};
use flare_im_core_sdk::{
    interface::event::SessionEventSubscriber,
    domain::event::*,
};
use anyhow::Result as AnyhowResult;

/// 会话事件订阅器（转发到前端）
pub struct SessionEventForwarder {
    app: AppHandle,
}

impl SessionEventForwarder {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl SessionEventSubscriber for SessionEventForwarder {
    async fn on_logged_in(&self, event: &SessionLoggedIn) -> AnyhowResult<()> {
        let _ = self.app.emit("im://session_logged_in", serde_json::json!({
            "user_id": event.user_id,
        }));
        Ok(())
    }

    async fn on_logged_out(&self, _event: &SessionLoggedOut) -> AnyhowResult<()> {
        let _ = self.app.emit("im://session_logged_out", serde_json::json!({}));
        Ok(())
    }

    async fn on_expired(&self, _event: &SessionExpired) -> AnyhowResult<()> {
        let _ = self.app.emit("im://session_expired", serde_json::json!({}));
        Ok(())
    }

    async fn on_token_refreshed(&self, event: &SessionTokenRefreshed) -> AnyhowResult<()> {
        let _ = self.app.emit("im://token_refreshed", serde_json::json!({
            "new_token": event.new_token,
        }));
        Ok(())
    }
}
