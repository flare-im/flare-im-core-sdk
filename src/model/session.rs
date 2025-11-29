//! 会话模型
//!
//! 封装 flare-proto 的 Session 相关类型，提供更友好的 API
//!
//! 设计原则：
//! 1. 基础会话结构直接使用 flare-proto（与服务端一致）
//! 2. SDK 扩展信息通过 ExtendedSessionSummary 添加（头像、置顶、免打扰等）

pub use flare_proto::session::{
    SessionSummary as SessionSummaryProto,
    SessionPolicy,
    DevicePresence as SessionDevicePresence,
    DeviceState as SessionDeviceState,
    ConflictResolution as SessionConflictResolution,
    SortOrder as SessionSortOrder,
};

use crate::model::extension::SessionExtension;

/// 会话摘要（用于会话列表展示）
/// 
/// 注意：此结构用于与服务端交互，SDK 内部使用 ExtendedSessionSummary
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    /// 会话 ID
    pub session_id: String,
    /// 会话类型（single/group/channel）
    pub session_type: String,
    /// 业务类型
    pub business_type: String,
    /// 最后一条消息 ID
    pub last_message_id: Option<String>,
    /// 最后一条消息时间（毫秒时间戳）
    pub last_message_time: Option<i64>,
    /// 最后发送者 ID
    pub last_sender_id: Option<String>,
    /// 最后消息类型
    pub last_message_type: i32,
    /// 最后内容类型
    pub last_content_type: String,
    /// 未读数
    pub unread_count: i32,
    /// 元数据
    pub metadata: std::collections::HashMap<String, String>,
    /// 服务器游标时间戳（毫秒）
    pub server_cursor_ts: Option<i64>,
    /// 显示名称
    pub display_name: Option<String>,
}

impl From<SessionSummaryProto> for SessionSummary {
    fn from(proto: SessionSummaryProto) -> Self {
        Self {
            session_id: proto.session_id,
            session_type: proto.session_type,
            business_type: proto.business_type,
            last_message_id: if proto.last_message_id.is_empty() { None } else { Some(proto.last_message_id) },
            last_message_time: proto.last_message_time
                .map(|ts| ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000)),
            last_sender_id: if proto.last_sender_id.is_empty() { None } else { Some(proto.last_sender_id) },
            last_message_type: proto.last_message_type as i32,
            last_content_type: proto.last_content_type,
            unread_count: proto.unread_count,
            metadata: proto.metadata,
            server_cursor_ts: if proto.server_cursor_ts == 0 { None } else { Some(proto.server_cursor_ts) },
            display_name: if proto.display_name.is_empty() { None } else { Some(proto.display_name) },
        }
    }
}

impl From<SessionSummary> for SessionSummaryProto {
    fn from(summary: SessionSummary) -> Self {
        use prost_types::Timestamp;
        
        Self {
            session_id: summary.session_id,
            session_type: summary.session_type,
            business_type: summary.business_type,
            last_message_id: summary.last_message_id.unwrap_or_default(),
            last_message_time: summary.last_message_time.map(|ts| {
                Timestamp {
                    seconds: ts / 1000,
                    nanos: ((ts % 1000) * 1_000_000) as i32,
                }
            }),
            last_sender_id: summary.last_sender_id.unwrap_or_default(),
            last_message_type: summary.last_message_type,
            last_content_type: summary.last_content_type,
            unread_count: summary.unread_count,
            metadata: summary.metadata,
            server_cursor_ts: summary.server_cursor_ts.unwrap_or(0),
            display_name: summary.display_name.unwrap_or_default(),
        }
    }
}

/// 带扩展的会话摘要（SDK 使用）
/// 
/// 包含基础会话摘要（来自 flare-proto）和 SDK 扩展信息（头像、置顶、免打扰等）
#[derive(Debug, Clone)]
pub struct ExtendedSessionSummary {
    /// 基础会话摘要（来自 flare-proto）
    pub session: SessionSummary,
    
    /// SDK 扩展信息
    pub extension: SessionExtension,
}

impl ExtendedSessionSummary {
    /// 从 SessionSummary 创建，扩展信息为空
    pub fn from_session(session: SessionSummary) -> Self {
        Self {
            session,
            extension: SessionExtension::default(),
        }
    }
    
    /// 从 SessionSummary 和 Extension 创建
    pub fn new(session: SessionSummary, extension: SessionExtension) -> Self {
        Self { session, extension }
    }
    
    /// 获取显示名称（优先使用扩展字段）
    pub fn display_name(&self) -> Option<&str> {
        self.extension.display_name.as_deref()
            .or_else(|| self.session.display_name.as_deref())
    }
    
    /// 获取头像 URL
    pub fn avatar(&self) -> Option<&str> {
        self.extension.avatar.as_deref()
    }
    
    /// 是否置顶
    pub fn is_pinned(&self) -> bool {
        self.extension.is_pinned
    }
    
    /// 是否免打扰
    pub fn is_muted(&self) -> bool {
        self.extension.is_muted
    }
    
    /// 设置显示名称
    pub fn set_display_name(&mut self, name: Option<String>) {
        self.extension.display_name = name;
    }
    
    /// 设置头像
    pub fn set_avatar(&mut self, avatar: Option<String>) {
        self.extension.avatar = avatar;
    }
    
    /// 设置置顶状态
    pub fn set_pinned(&mut self, pinned: bool) {
        self.extension.is_pinned = pinned;
    }
    
    /// 设置免打扰状态
    pub fn set_muted(&mut self, muted: bool) {
        self.extension.is_muted = muted;
    }
    
    /// 更新最后查看时间
    pub fn update_last_viewed(&mut self) {
        self.extension.last_viewed_at = Some(chrono::Utc::now().timestamp_millis());
    }
    
    /// 获取自定义扩展字段
    pub fn get_custom_field(&self, key: &str) -> Option<&String> {
        self.extension.custom.get(key)
    }
    
    /// 设置自定义扩展字段
    pub fn set_custom_field(&mut self, key: String, value: String) {
        self.extension.custom.insert(key, value);
    }
}

impl From<SessionSummary> for ExtendedSessionSummary {
    fn from(session: SessionSummary) -> Self {
        Self::from_session(session)
    }
}

impl From<ExtendedSessionSummary> for SessionSummary {
    fn from(extended: ExtendedSessionSummary) -> Self {
        extended.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_summary_conversion() {
        let mut proto = SessionSummaryProto::default();
        proto.session_id = "session-123".to_string();
        proto.session_type = "single".to_string();
        proto.unread_count = 5;
        
        let summary: SessionSummary = proto.clone().into();
        assert_eq!(summary.session_id, proto.session_id);
        assert_eq!(summary.unread_count, 5);
        
        let proto2: SessionSummaryProto = summary.into();
        assert_eq!(proto2.session_id, proto.session_id);
    }
}
