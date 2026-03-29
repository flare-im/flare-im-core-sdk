//! 同步响应处理：由 application 实现，Dispatcher 收到服务端包时回调。
//! 与 flare-proto 对齐：SyncRes（common）。

use std::pin::Pin;

use flare_proto::common::SyncRes;

/// 同步协议响应处理（会话列表 / 单会话消息）
///
/// 由 [crate::application::handlers::SyncHandler] 实现，Dispatcher 分发 SyncResp / SyncConversationsResp 时调用。
pub trait SyncResponseHandler: Send + Sync {
    fn handle_sync_response(
        &self,
        resp: SyncRes,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>;
}
