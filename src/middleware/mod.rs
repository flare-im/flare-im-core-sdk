pub mod retry;
pub mod auth;
pub mod logging;

use std::sync::Arc;
use async_trait::async_trait;
use tracing::warn;

use crate::error::Result;
use crate::event::SdkEvent;

pub use retry::RetryMiddleware;
pub use auth::AuthMiddleware;
pub use logging::LoggingMiddleware;

// ── 拦截器 traits ────────────────────────────────────────────

/// 消息拦截器 — 收发管道中变换消息
#[async_trait]
pub trait MessageInterceptor: Send + Sync {
    fn name(&self) -> &str;
    async fn on_incoming(&self, message: flare_proto::common::Message) -> Result<Option<flare_proto::common::Message>> {
        Ok(Some(message))
    }
    async fn on_outgoing(&self, message: flare_proto::common::Message) -> Result<flare_proto::common::Message> {
        Ok(message)
    }
}

/// 事件拦截器 — 事件广播前拦截/变换
#[async_trait]
pub trait EventInterceptor: Send + Sync {
    fn name(&self) -> &str;
    async fn on_event(&self, event: SdkEvent) -> Result<Option<SdkEvent>> {
        Ok(Some(event))
    }
    async fn on_custom_push(
        &self,
        _data_type: &str,
        _payload: &[u8],
        _metadata: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<SdkEvent>> {
        Ok(Vec::new())
    }
}

// ── MiddlewareChain ──────────────────────────────────────────

/// 中间件链 — 聚合所有拦截器
pub struct MiddlewareChain {
    message_interceptors: Vec<Arc<dyn MessageInterceptor>>,
    event_interceptors: Vec<Arc<dyn EventInterceptor>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            message_interceptors: Vec::new(),
            event_interceptors: Vec::new(),
        }
    }

    pub fn add_message_interceptor(&mut self, i: Arc<dyn MessageInterceptor>) {
        self.message_interceptors.push(i);
    }

    pub fn add_event_interceptor(&mut self, i: Arc<dyn EventInterceptor>) {
        self.event_interceptors.push(i);
    }

    pub fn has_message_interceptors(&self) -> bool { !self.message_interceptors.is_empty() }
    pub fn has_event_interceptors(&self) -> bool { !self.event_interceptors.is_empty() }

    pub async fn intercept_incoming(
        &self,
        mut message: flare_proto::common::Message,
    ) -> Result<Option<flare_proto::common::Message>> {
        for i in &self.message_interceptors {
            match i.on_incoming(message).await {
                Ok(Some(m)) => message = m,
                Ok(None) => return Ok(None),
                Err(e) => { warn!(interceptor = i.name(), error = %e, "incoming interceptor failed"); return Ok(None); }
            }
        }
        Ok(Some(message))
    }

    pub async fn intercept_outgoing(
        &self,
        mut message: flare_proto::common::Message,
    ) -> Result<flare_proto::common::Message> {
        for i in &self.message_interceptors {
            message = i.on_outgoing(message).await?;
        }
        Ok(message)
    }

    pub async fn intercept_event(&self, mut event: SdkEvent) -> Result<Option<SdkEvent>> {
        for i in &self.event_interceptors {
            match i.on_event(event).await {
                Ok(Some(e)) => event = e,
                Ok(None) => return Ok(None),
                Err(e) => { warn!(interceptor = i.name(), error = %e, "event interceptor failed"); return Ok(None); }
            }
        }
        Ok(Some(event))
    }

    pub async fn handle_custom_push(
        &self,
        data_type: &str,
        payload: &[u8],
        metadata: &std::collections::HashMap<String, String>,
    ) -> Vec<SdkEvent> {
        let mut events = Vec::new();
        for i in &self.event_interceptors {
            if let Ok(evts) = i.on_custom_push(data_type, payload, metadata).await {
                events.extend(evts);
            }
        }
        events
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self { Self::new() }
}
