use std::sync::Arc;

use crate::event::MessageEvent;
use crate::store::MessageStore;

/// 消息推送处理器 — 监听 EventBus 中的消息事件并执行副作用
///
/// 职责：
/// - 新消息持久化
/// - 撤回/删除状态更新
/// - ACK 确认（可选）
pub struct MessageHandler {
    store: Arc<dyn MessageStore>,
}

impl MessageHandler {
    pub fn new(store: Arc<dyn MessageStore>) -> Self {
        Self { store }
    }

    /// 处理单条消息事件
    pub async fn handle(&self, event: &MessageEvent) {
        match event {
            MessageEvent::Received { message } => {
                if let Err(e) = self.store.save_batch(&[message.clone()]).await {
                    tracing::warn!(error = %e, "save message failed");
                }
            }
            MessageEvent::Recalled { event: recall, .. } => {
                let _ = self.store.update_status(
                    &recall.server_msg_id,
                    flare_proto::common::MessageStatus::Recalled as i32,
                ).await;
            }
            MessageEvent::Deleted { event: del, .. } => {
                let _ = self.store.delete(&del.server_msg_id).await;
            }
            _ => {}
        }
    }
}
