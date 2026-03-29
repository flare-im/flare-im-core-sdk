//! 插件扩展（Plugin）
//!
//! 加密、日志、统计等可插拔能力，通过消息/事件拦截器接入，不污染核心逻辑。

use std::sync::Arc;

/// 消息拦截器（发送前/接收后）
pub trait MessageInterceptor: Send + Sync {}

/// 事件拦截器
pub trait EventInterceptor: Send + Sync {}

/// 拦截器链（可扩展）
pub struct MiddlewareChain {
    _message: Vec<Arc<dyn MessageInterceptor>>,
    _event: Vec<Arc<dyn EventInterceptor>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        Self {
            _message: Vec::new(),
            _event: Vec::new(),
        }
    }

    pub fn add_message_interceptor(&mut self, _i: Arc<dyn MessageInterceptor>) {
        // self._message.push(i);
    }

    pub fn add_event_interceptor(&mut self, _i: Arc<dyn EventInterceptor>) {
        // self._event.push(i);
    }
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}
