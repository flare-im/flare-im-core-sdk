//! SDK 常量定义
//!
//! 包含所有业务常量，方便使用和维护

/// 消息相关常量
pub mod message {
    /// 默认消息撤回时间限制（秒）
    pub const DEFAULT_RECALL_TIME_LIMIT_SECONDS: i32 = 120; // 2 分钟
    
    /// 最大消息撤回时间限制（秒）
    pub const MAX_RECALL_TIME_LIMIT_SECONDS: i32 = 300; // 5 分钟
    
    /// 消息内容最大长度（字节）
    pub const MAX_MESSAGE_CONTENT_LENGTH: usize = 10 * 1024 * 1024; // 10MB
    
    /// 文本消息最大长度（字符）
    pub const MAX_TEXT_MESSAGE_LENGTH: usize = 10000;
    
    /// 消息 ID 最大长度
    pub const MAX_MESSAGE_ID_LENGTH: usize = 128;
    
    /// 客户端消息 ID 最大长度
    pub const MAX_CLIENT_MSG_ID_LENGTH: usize = 128;
    
    /// 消息去重窗口（秒）
    pub const MESSAGE_DEDUP_WINDOW_SECONDS: i64 = 60;
    
    /// 消息重试最大次数
    pub const MAX_MESSAGE_RETRY_COUNT: u32 = 3;
    
    /// 消息发送超时（秒）
    pub const MESSAGE_SEND_TIMEOUT_SECONDS: u64 = 30;
}

/// 会话相关常量
pub mod conversation {
    /// 会话列表默认分页大小
    pub const DEFAULT_PAGE_SIZE: usize = 20;
    
    /// 会话列表最大分页大小
    pub const MAX_PAGE_SIZE: usize = 100;
    
    /// 会话草稿最大长度（字符）
    pub const MAX_DRAFT_LENGTH: usize = 5000;
    
    /// 会话名称最大长度（字符）
    pub const MAX_CONVERSATION_NAME_LENGTH: usize = 100;
    
    /// 会话描述最大长度（字符）
    pub const MAX_CONVERSATION_DESCRIPTION_LENGTH: usize = 500;
    
    /// 输入状态超时（毫秒）
    pub const INPUT_STATE_TIMEOUT_MS: u64 = 30000; // 30 秒
}

/// 连接相关常量
pub mod connection {
    /// 心跳间隔（秒）
    pub const HEARTBEAT_INTERVAL_SECONDS: u64 = 30;
    
    /// 心跳超时（秒）
    pub const HEARTBEAT_TIMEOUT_SECONDS: u64 = 60;
    
    /// 连接超时（秒）
    pub const CONNECTION_TIMEOUT_SECONDS: u64 = 10;
    
    /// 重连最大延迟（秒）
    pub const MAX_RECONNECT_DELAY_SECONDS: u64 = 60;
    
    /// 重连初始延迟（秒）
    pub const INITIAL_RECONNECT_DELAY_SECONDS: u64 = 1;
    
    /// 最大重连次数
    pub const MAX_RECONNECT_ATTEMPTS: u32 = 10;
}

/// 同步相关常量
pub mod sync {
    /// Bootstrap Sync 超时（秒）
    pub const BOOTSTRAP_SYNC_TIMEOUT_SECONDS: u64 = 60;
    
    /// Async Sync 超时（秒）
    pub const ASYNC_SYNC_TIMEOUT_SECONDS: u64 = 30;
    
    /// 同步批次大小
    pub const SYNC_BATCH_SIZE: usize = 100;
    
    /// 同步重试最大次数
    pub const MAX_SYNC_RETRY_COUNT: u32 = 3;
}

/// 存储相关常量
pub mod storage {
    /// 默认缓存大小（字节）
    pub const DEFAULT_CACHE_SIZE: u64 = 1024 * 1024 * 1024; // 1GB
    
    /// 最大缓存大小（字节）
    pub const MAX_CACHE_SIZE: u64 = 10 * 1024 * 1024 * 1024; // 10GB
    
    /// 消息队列默认容量
    pub const DEFAULT_MESSAGE_QUEUE_CAPACITY: usize = 1000;
    
    /// 消息队列最大容量
    pub const MAX_MESSAGE_QUEUE_CAPACITY: usize = 10000;
    
    /// 事件存储批次大小
    pub const EVENT_STORE_BATCH_SIZE: usize = 100;
}

/// 媒体相关常量
pub mod media {
    /// 图片最大大小（字节）
    pub const MAX_IMAGE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
    
    /// 视频最大大小（字节）
    pub const MAX_VIDEO_SIZE: u64 = 100 * 1024 * 1024; // 100MB
    
    /// 音频最大大小（字节）
    pub const MAX_AUDIO_SIZE: u64 = 10 * 1024 * 1024; // 10MB
    
    /// 文件最大大小（字节）
    pub const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024; // 500MB
    
    /// 支持的图片格式
    pub const SUPPORTED_IMAGE_FORMATS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];
    
    /// 支持的视频格式
    pub const SUPPORTED_VIDEO_FORMATS: &[&str] = &["mp4", "mov", "avi", "mkv"];
    
    /// 支持的音频格式
    pub const SUPPORTED_AUDIO_FORMATS: &[&str] = &["mp3", "m4a", "ogg", "wav"];
}

/// 限流相关常量
pub mod rate_limit {
    /// 消息发送速率限制（条/秒）
    pub const MESSAGE_SEND_RATE_LIMIT: u32 = 10;
    
    /// 消息发送突发限制（条）
    pub const MESSAGE_SEND_BURST_LIMIT: u32 = 20;
    
    /// API 调用速率限制（次/秒）
    pub const API_CALL_RATE_LIMIT: u32 = 100;
}

/// 错误码常量
pub mod error_code {
    /// 成功
    pub const SUCCESS: i32 = 0;
    
    /// 未知错误
    pub const UNKNOWN_ERROR: i32 = 1000;
    
    /// 网络错误
    pub const NETWORK_ERROR: i32 = 1001;
    
    /// 认证失败
    pub const AUTH_FAILED: i32 = 1002;
    
    /// 消息格式错误
    pub const INVALID_MESSAGE_FORMAT: i32 = 1003;
    
    /// 消息发送失败
    pub const MESSAGE_SEND_FAILED: i32 = 1004;
    
    /// 消息撤回失败
    pub const MESSAGE_RECALL_FAILED: i32 = 1005;
    
    /// 会话不存在
    pub const CONVERSATION_NOT_FOUND: i32 = 1006;
    
    /// 权限不足
    pub const PERMISSION_DENIED: i32 = 1007;
    
    /// 参数错误
    pub const INVALID_PARAMETER: i32 = 1008;
    
    /// 超时
    pub const TIMEOUT: i32 = 1009;
}
