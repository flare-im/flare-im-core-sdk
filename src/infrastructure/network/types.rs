//! 网络模块类型定义
//!
//! 定义网络层使用的消息和事件类型

use flare_core::common::protocol::Frame;
use flare_proto::common::{
    EventEnvelope, SyncResponse, SyncConversationsResponse,
    ConversationSyncAllResponse, GetConversationDetailResponse,
};

/// 网络消息
///
/// 用于在网络层和应用层之间传递消息和数据
///
/// # 消息类型分类
///
/// - **EventEnvelope** - 事件批（推送/同步，以 proto Event 为唯一数据源）
/// - **Received** - 非 ServerPacket 的原始 Frame（向后兼容）
/// - **SyncMessages** - 消息同步响应（批量处理）
/// - **SyncConversations** - 会话增量同步（补丁处理）
/// - **ConversationSyncAll** - 全量会话同步
/// - **ConversationDetail** - 会话详情响应
/// - **CustomPushData** - 自定义推送数据
#[derive(Debug, Clone)]
pub enum NetworkMessage {
    /// 事件批（ServerPacket.event_envelope）
    ///
    /// 统一走事件流处理：按 EventType 分发入队/发布领域事件
    EventEnvelope(EventEnvelope),

    /// 收到的消息 Frame（非 ServerPacket 或未知类型时使用）
    ///
    /// 需要解析 MessageEnvelope 提取消息
    Received(Frame),
    
    /// 消息同步响应
    ///
    /// 包含 MessageEnvelope，需要批量处理
    SyncMessages(SyncResponse),
    
    /// 会话增量同步响应
    ///
    /// 包含会话补丁列表，需要按 patch_type 分类处理
    SyncConversations(SyncConversationsResponse),
    
    /// 全量会话同步响应
    ///
    /// 包含完整会话列表，需要批量更新
    ConversationSyncAll(ConversationSyncAllResponse),
    
    /// 会话详情响应
    ///
    /// 包含会话详情，需要更新本地会话
    ConversationDetail(GetConversationDetailResponse),
    
    /// 收到的自定义推送数据
    CustomPushData {
        /// 自定义数据类型标识
        data_type: String,
        /// 二进制负载
        payload: Vec<u8>,
        /// 元数据
        metadata: std::collections::HashMap<String, String>,
    },
    
    /// 连接成功
    Connected(String), // connection_id
    
    /// 连接断开
    Disconnected(String), // reason
    
    /// 连接错误
    Error(String),
}

/// 连接事件
///
/// 用于通知连接状态变化
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    /// 连接已建立
    Connected,
    
    /// 连接已断开
    Disconnected,
    
    /// 连接错误
    Error(String),
}
