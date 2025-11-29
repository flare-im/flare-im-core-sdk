//! 同步模型
//!
//! 定义消息同步相关的数据模型

use serde::{Deserialize, Serialize};

/// 同步游标（用于增量同步）
/// 
/// 采用分层同步策略（层级化游标）：
/// - 层级1：只同步最近N条消息（给UI使用）
/// - 层级2：同步游标信息（max_seq, cursor_seq, unread_count），而不是所有消息实体
/// - 层级3：按需加载历史消息（用户手动下拉时才加载）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncCursor {
    /// 会话 ID
    pub session_id: String,
    /// 最后同步的消息序列号（客户端已同步到的seq，即 cursor_seq）
    pub last_seq: Option<i64>,
    /// 最后同步的时间戳（毫秒）
    pub last_timestamp: Option<i64>,
    /// 最后同步的消息 ID
    pub last_message_id: Option<String>,
    /// 服务器最大序列号（max_seq，从服务器获取）
    /// 用于计算未读数：unread_count = max_seq - cursor_seq
    pub max_seq: Option<i64>,
    /// 未读消息数量（从服务器获取，或计算：max_seq - cursor_seq）
    pub unread_count: Option<i64>,
    /// 是否已同步最近消息（层级1：最近N条消息）
    pub recent_messages_synced: bool,
    /// 最近消息同步的序列号范围（用于层级1）
    /// 例如：如果同步了最近200条，范围是 [max_seq - 199, max_seq]
    pub recent_sync_range: Option<(i64, i64)>,  // (start_seq, end_seq)
}

impl SyncCursor {
    /// 创建新的同步游标
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            last_seq: None,
            last_timestamp: None,
            last_message_id: None,
            max_seq: None,
            unread_count: None,
            recent_messages_synced: false,
            recent_sync_range: None,
        }
    }
    
    /// 更新游标（基础信息）
    pub fn update(&mut self, seq: Option<i64>, timestamp: Option<i64>, message_id: Option<String>) {
        if let Some(s) = seq {
            self.last_seq = Some(s);
        }
        if let Some(t) = timestamp {
            self.last_timestamp = Some(t);
        }
        if let Some(id) = message_id {
            self.last_message_id = Some(id);
        }
    }
    
    /// 更新服务器游标信息（层级2：游标信息）
    /// 
    /// # 参数
    /// - `max_seq`: 服务器最大序列号
    /// - `unread_count`: 未读消息数量（如果服务器提供）
    pub fn update_server_cursor(&mut self, max_seq: i64, unread_count: Option<i64>) {
        self.max_seq = Some(max_seq);
        
        // 计算未读数：如果服务器未提供，则计算 max_seq - cursor_seq
        if let Some(count) = unread_count {
            self.unread_count = Some(count);
        } else if let Some(cursor_seq) = self.last_seq {
            self.unread_count = Some((max_seq - cursor_seq).max(0));
        }
    }
    
    /// 更新最近消息同步范围（层级1：最近N条消息）
    /// 
    /// # 参数
    /// - `start_seq`: 开始序列号
    /// - `end_seq`: 结束序列号（通常是 max_seq）
    pub fn update_recent_sync_range(&mut self, start_seq: i64, end_seq: i64) {
        self.recent_sync_range = Some((start_seq, end_seq));
        self.recent_messages_synced = true;
        // 更新 last_seq 为最近消息的结束序列号
        self.last_seq = Some(end_seq);
    }
    
    /// 获取未读消息数量
    /// 
    /// 优先使用服务器提供的 unread_count，否则计算 max_seq - cursor_seq
    pub fn get_unread_count(&self) -> i64 {
        if let Some(count) = self.unread_count {
            count
        } else if let (Some(max_seq), Some(cursor_seq)) = (self.max_seq, self.last_seq) {
            (max_seq - cursor_seq).max(0)
        } else {
            0
        }
    }
    
    /// 检查是否需要同步历史消息（层级3：按需加载）
    /// 
    /// 如果用户已同步到 max_seq，则不需要同步
    pub fn needs_history_sync(&self) -> bool {
        if let (Some(max_seq), Some(cursor_seq)) = (self.max_seq, self.last_seq) {
            cursor_seq < max_seq
        } else {
            false
        }
    }
    
    /// 获取需要同步的序列号范围
    /// 
    /// 返回 (start_seq, end_seq)，用于增量同步
    pub fn get_sync_range(&self) -> Option<(i64, i64)> {
        if let (Some(max_seq), Some(cursor_seq)) = (self.max_seq, self.last_seq) {
            if cursor_seq < max_seq {
                Some((cursor_seq + 1, max_seq))
            } else {
                None
            }
        } else {
            None
        }
    }
}

/// 同步结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// 会话 ID
    pub session_id: String,
    /// 同步的消息数量
    pub message_count: usize,
    /// 是否有更多消息
    pub has_more: bool,
    /// 下一个游标
    pub next_cursor: Option<SyncCursor>,
    /// 最后一条消息的序列号
    pub last_seq: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sync_cursor() {
        let mut cursor = SyncCursor::new("session-123".to_string());
        cursor.update(Some(100), Some(1234567890), Some("msg-456".to_string()));
        
        assert_eq!(cursor.last_seq, Some(100));
        assert_eq!(cursor.last_timestamp, Some(1234567890));
        assert_eq!(cursor.last_message_id, Some("msg-456".to_string()));
    }
}
