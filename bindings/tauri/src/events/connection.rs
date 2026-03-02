//! 连接事件转发
//!
//! 将 SDK 连接事件自动转发到 Tauri 前端

use tauri::{AppHandle, Emitter};
use flare_im_core_sdk::{
    interface::event::ConnectionEventSubscriber,
    domain::event::*,
};
use anyhow::Result as AnyhowResult;

/// 连接事件订阅器（转发到前端）
pub struct ConnectionEventForwarder {
    app: AppHandle,
}

impl ConnectionEventForwarder {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl ConnectionEventSubscriber for ConnectionEventForwarder {
    async fn on_connected(&self, event: &ConnectionConnected) -> AnyhowResult<()> {
        let _ = self.app.emit("im://connection_state", serde_json::json!({
            "state": "Connected",
            "connected": true,
            "connection_id": event.connection_id,
        }));
        Ok(())
    }

    async fn on_disconnected(&self, event: &ConnectionDisconnected) -> AnyhowResult<()> {
        let _ = self.app.emit("im://connection_state", serde_json::json!({
            "state": "Disconnected",
            "connected": false,
            "reason": event.reason,
        }));
        Ok(())
    }

    async fn on_reconnecting(&self, event: &ConnectionReconnecting) -> AnyhowResult<()> {
        let _ = self.app.emit("im://connection_state", serde_json::json!({
            "state": "Reconnecting",
            "connected": false,
            "attempt": event.attempt,
        }));
        Ok(())
    }

    async fn on_reconnected(&self, event: &ConnectionReconnected) -> AnyhowResult<()> {
        let _ = self.app.emit("im://connection_state", serde_json::json!({
            "state": "Reconnected",
            "connected": true,
            "connection_id": event.connection_id,
            "attempt": event.attempt,
        }));
        Ok(())
    }

    async fn on_connect_failed(&self, event: &ConnectionConnectFailed) -> AnyhowResult<()> {
        let _ = self.app.emit("im://connection_state", serde_json::json!({
            "state": "ConnectFailed",
            "connected": false,
            "error": event.error,
            "attempt": event.attempt,
        }));
        Ok(())
    }
}
