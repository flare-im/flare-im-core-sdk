//! 同步事件转发
//!
//! 将 SDK 同步事件（Bootstrap、增量同步等）自动转发到 Tauri 前端

use tauri::{AppHandle, Emitter};
use flare_im_core_sdk::{
    interface::event::SyncEventSubscriber,
    domain::event::*,
};
use anyhow::Result as AnyhowResult;

/// 同步事件订阅器（转发到前端）
pub struct SyncEventForwarder {
    app: AppHandle,
}

impl SyncEventForwarder {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl SyncEventSubscriber for SyncEventForwarder {
    async fn on_bootstrap_started(&self, _event: &SyncBootstrapStarted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_bootstrap_started", serde_json::json!({}));
        Ok(())
    }

    async fn on_bootstrap_completed(&self, _event: &SyncBootstrapCompleted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_bootstrap_completed", serde_json::json!({}));
        Ok(())
    }

    async fn on_bootstrap_failed(&self, event: &SyncBootstrapFailed) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_bootstrap_failed", serde_json::json!({
            "error": event.error,
        }));
        Ok(())
    }

    async fn on_async_started(&self, event: &SyncAsyncStarted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_async_started", serde_json::json!({
            "sync_type": event.sync_type,
        }));
        Ok(())
    }

    async fn on_async_completed(&self, event: &SyncAsyncCompleted) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_async_completed", serde_json::json!({
            "sync_type": event.sync_type,
        }));
        Ok(())
    }

    async fn on_async_failed(&self, event: &SyncAsyncFailed) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_async_failed", serde_json::json!({
            "sync_type": event.sync_type,
            "error": event.error,
        }));
        Ok(())
    }

    async fn on_progress_updated(&self, event: &SyncProgressUpdated) -> AnyhowResult<()> {
        let _ = self.app.emit("im://sync_progress_updated", serde_json::json!({
            "progress": event.progress,
            "total": event.total,
        }));
        Ok(())
    }
}
