//! 客户端配置管理

use flare_core::common::config_types::TransportProtocol;
#[cfg(target_arch = "wasm32")]
use js_sys::{Date, Math};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use uuid::Uuid;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// 设备平台
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DevicePlatform {
    Web,
    Android,
    IOS,
    HarmonyOS,
    Desktop,
}

impl Default for DevicePlatform {
    fn default() -> Self {
        // 自动检测平台
        #[cfg(target_arch = "wasm32")]
        return DevicePlatform::Web;

        #[cfg(target_os = "android")]
        return DevicePlatform::Android;

        #[cfg(target_os = "ios")]
        return DevicePlatform::IOS;

        #[cfg(target_os = "harmonyos")]
        return DevicePlatform::HarmonyOS;

        #[cfg(not(any(
            target_arch = "wasm32",
            target_os = "android",
            target_os = "ios",
            target_os = "harmonyos"
        )))]
        return DevicePlatform::Desktop;

        #[allow(unreachable_code)]
        DevicePlatform::Desktop
    }
}

impl From<crate::shared::platform::Platform> for DevicePlatform {
    fn from(platform: crate::shared::platform::Platform) -> Self {
        match platform {
            crate::shared::platform::Platform::Web => DevicePlatform::Web,
            crate::shared::platform::Platform::Desktop => DevicePlatform::Desktop,
            crate::shared::platform::Platform::Android => DevicePlatform::Android,
            crate::shared::platform::Platform::IOS => DevicePlatform::IOS,
            crate::shared::platform::Platform::HarmonyOS => DevicePlatform::HarmonyOS,
        }
    }
}

impl From<DevicePlatform> for crate::shared::platform::Platform {
    fn from(platform: DevicePlatform) -> Self {
        match platform {
            DevicePlatform::Web => crate::shared::platform::Platform::Web,
            DevicePlatform::Desktop => crate::shared::platform::Platform::Desktop,
            DevicePlatform::Android => crate::shared::platform::Platform::Android,
            DevicePlatform::IOS => crate::shared::platform::Platform::IOS,
            DevicePlatform::HarmonyOS => crate::shared::platform::Platform::HarmonyOS,
        }
    }
}

/// 客户端配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    /// 服务器 WebSocket URL
    pub server_url: String,

    /// 媒体服务 HTTP 端点（可选，用于文件上传/下载）
    /// 注意：媒体上传/下载使用 HTTP，而非长连接（详见架构设计文档）
    pub media_base_url: Option<String>,

    /// 传输协议（单个协议模式）
    /// 如果设置了 protocols，此字段将被忽略
    pub protocol: Option<TransportProtocol>,

    /// 传输协议列表（协议竞速模式）
    /// 列表顺序就是优先级顺序，前面的优先级更高
    /// 如果设置了此列表，protocol 将被忽略
    pub protocols: Option<Vec<TransportProtocol>>,

    /// 每个协议的独立地址配置（协议 -> 地址映射）
    /// 如果设置了此映射，每个协议将使用对应的地址
    pub protocol_urls: Option<HashMap<TransportProtocol, String>>,

    /// 协议竞速超时时间（如果启用多协议竞速）
    pub race_timeout: Option<Duration>,

    /// 用户ID
    pub user_id: String,

    /// 设备ID
    pub device_id: String,

    /// 设备平台
    pub device_platform: DevicePlatform,

    /// 应用版本
    pub app_version: Option<String>,

    /// 连接超时（秒）
    pub connect_timeout: u64,

    /// 心跳间隔（秒）
    pub heartbeat_interval: u64,

    /// 重连间隔（秒）
    pub reconnect_interval: u64,

    /// 最大重连次数（0表示无限制）
    pub max_reconnect_attempts: u32,

    /// 是否自动重连
    pub auto_reconnect: bool,

    /// 租户ID（可选）
    pub tenant_id: Option<String>,

    /// Token（用于认证，如果服务端启用认证，必须提供）
    pub token: Option<String>,

    /// 缓存配置（可选）
    pub cache_config: Option<CacheConfig>,

    /// 消息状态机配置（可选）
    pub message_state_machine_config: Option<MessageStateMachineConfig>,

    /// 待发送消息队列配置（可选）
    pub pending_message_queue_config: Option<PendingMessageQueueConfig>,

    /// 数据库路径（可选，用于 SQLite 存储）
    /// 如果未设置，将从环境变量 FLARE_IM_DB_PATH 读取，或使用默认值 "flare-im.db"
    pub db_path: Option<String>,
}

/// 缓存配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// 是否启用查询缓存（默认：true）
    pub enabled: bool,

    /// 消息缓存 TTL（秒，默认：300）
    pub message_ttl: u64,

    /// 会话缓存 TTL（秒，默认：180）
    pub session_ttl: u64,

    /// 消息列表缓存 TTL（秒，默认：60）
    pub message_list_ttl: u64,

    /// 会话列表缓存 TTL（秒，默认：30）
    pub session_list_ttl: u64,

    /// 缓存清理间隔（秒，默认：300）
    pub cleanup_interval: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            message_ttl: 300,      // 5 分钟
            session_ttl: 180,      // 3 分钟
            message_list_ttl: 60,  // 1 分钟
            session_list_ttl: 30,  // 30 秒
            cleanup_interval: 300, // 5 分钟
        }
    }
}

/// 消息状态机配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStateMachineConfig {
    /// 是否启用状态机（默认：true）
    pub enabled: bool,

    /// 状态转换超时时间（秒，默认：30）
    pub state_timeout: u64,

    /// 是否允许快速转换（跳过中间状态，默认：true）
    pub allow_fast_transition: bool,

    /// 失败重试次数（默认：3）
    pub max_retries: u32,

    /// 失败重试延迟（秒，默认：1）
    pub retry_delay: u64,
}

impl Default for MessageStateMachineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            state_timeout: 30,
            allow_fast_transition: true,
            max_retries: 3,
            retry_delay: 1,
        }
    }
}

/// 待发送消息队列配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMessageQueueConfig {
    /// 是否启用持久化（默认：true）
    pub enable_persistence: bool,

    /// 最大队列大小（默认：1000）
    pub max_queue_size: usize,

    /// 最大重试次数（默认：3）
    pub max_retries: u32,

    /// 重试延迟（秒，默认：1）
    pub retry_delay: u64,

    /// 重试延迟倍数（指数退避，默认：2.0）
    pub retry_delay_multiplier: f64,

    /// 最大重试延迟（秒，默认：60）
    pub max_retry_delay: u64,

    /// 是否启用去重（默认：true）
    pub enable_deduplication: bool,

    /// 去重窗口时间（秒，默认：24小时）
    pub dedup_window: u64,

    /// 清理间隔（秒，默认：1小时）
    pub cleanup_interval: u64,

    /// 已完成消息保留时间（秒，默认：7天）
    pub completed_retention: u64,
}

impl Default for PendingMessageQueueConfig {
    fn default() -> Self {
        Self {
            enable_persistence: true,
            max_queue_size: 1000,
            max_retries: 3,
            retry_delay: 1,
            retry_delay_multiplier: 2.0,
            max_retry_delay: 60,
            enable_deduplication: true,
            dedup_window: 24 * 3600,
            cleanup_interval: 3600,
            completed_retention: 7 * 24 * 3600,
        }
    }
}

impl ClientConfig {
    /// 创建配置构建器
    pub fn builder() -> ClientConfigBuilder {
        ClientConfigBuilder::default()
    }

    /// 从环境变量加载配置
    pub fn from_env() -> anyhow::Result<Self> {
        let mut builder = ClientConfigBuilder::default()
            .server_url(
                std::env::var("FLARE_IM_SERVER_URL")
                    .unwrap_or_else(|_| "ws://localhost:60051".to_string()),
            )
            .user_id(std::env::var("FLARE_IM_USER_ID")?)
            .device_id(std::env::var("FLARE_IM_DEVICE_ID").unwrap_or_else(|_| default_device_id()));

        // 如果设置了媒体服务地址，则添加
        if let Ok(media_url) = std::env::var("FLARE_IM_MEDIA_BASE_URL") {
            builder = builder.media_base_url(media_url);
        }

        builder.build()
    }

    /// 验证配置
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.server_url.is_empty() {
            return Err(anyhow::anyhow!("server_url cannot be empty"));
        }

        if self.user_id.is_empty() {
            return Err(anyhow::anyhow!("user_id cannot be empty"));
        }

        if self.device_id.is_empty() {
            return Err(anyhow::anyhow!("device_id cannot be empty"));
        }

        // 验证 URL 格式
        if !self.server_url.starts_with("ws://")
            && !self.server_url.starts_with("wss://")
            && !self.server_url.starts_with("quic://")
        {
            return Err(anyhow::anyhow!(
                "server_url must start with ws://, wss://, or quic://"
            ));
        }

        // 验证媒体端点（如果提供）
        if let Some(ref endpoint) = self.media_base_url {
            if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
                return Err(anyhow::anyhow!(
                    "media_base_url must start with http:// or https://"
                ));
            }
        }

        Ok(())
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        use crate::shared::platform::{Platform, get_platform};
        let platform = get_platform();

        // 根据平台自动调整配置
        let (protocol, protocols, race_timeout, connect_timeout, heartbeat_interval) =
            match platform {
                Platform::Web => (
                    Some(TransportProtocol::WebSocket), // Web 仅支持 WebSocket
                    None,
                    None,
                    20, // Web 端连接超时较短
                    30,
                ),
                Platform::Desktop | Platform::Android | Platform::IOS | Platform::HarmonyOS => (
                    None,
                    Some(vec![
                        TransportProtocol::QUIC,      // 移动端和桌面端优先使用 QUIC
                        TransportProtocol::WebSocket, // WebSocket 作为备用
                    ]),
                    Some(Duration::from_secs(5)),
                    30,
                    30,
                ),
            };

        Self {
            server_url: "ws://localhost:60051/".to_string(),
            media_base_url: None,
            protocol,
            protocols,
            protocol_urls: None,
            race_timeout,
            user_id: String::new(),
            device_id: default_device_id(),
            device_platform: platform.into(),
            app_version: None,
            connect_timeout,
            heartbeat_interval,
            reconnect_interval: 5,
            max_reconnect_attempts: 0,
            auto_reconnect: true,
            tenant_id: None,
            token: None,
            cache_config: Some(CacheConfig::default()),
            message_state_machine_config: Some(MessageStateMachineConfig::default()),
            pending_message_queue_config: Some(PendingMessageQueueConfig::default()),
            db_path: None,
        }
    }
}

/// 配置构建器
#[derive(Default)]
pub struct ClientConfigBuilder {
    server_url: Option<String>,
    media_base_url: Option<String>,      // 媒体服务 HTTP 地址（可选）
    protocol: Option<TransportProtocol>, // 单个协议
    protocols: Option<Vec<TransportProtocol>>, // 协议列表（竞速）
    protocol_urls: Option<HashMap<TransportProtocol, String>>, // 协议地址映射
    race_timeout: Option<Duration>,      // 竞速超时
    user_id: Option<String>,
    device_id: Option<String>,
    device_platform: Option<DevicePlatform>,
    app_version: Option<String>,
    connect_timeout: Option<u64>,
    heartbeat_interval: Option<u64>,
    reconnect_interval: Option<u64>,
    max_reconnect_attempts: Option<u32>,
    auto_reconnect: Option<bool>,
    tenant_id: Option<String>,
    token: Option<String>,
    cache_config: Option<CacheConfig>,
    message_state_machine_config: Option<MessageStateMachineConfig>,
    pending_message_queue_config: Option<PendingMessageQueueConfig>,
    db_path: Option<String>,
}

impl ClientConfigBuilder {
    pub fn server_url(mut self, url: impl Into<String>) -> Self {
        self.server_url = Some(url.into());
        self
    }

    pub fn media_base_url(mut self, url: impl Into<String>) -> Self {
        self.media_base_url = Some(url.into());
        self
    }

    /// 设置媒体服务地址（可选）
    pub fn media_base_url_opt(mut self, url: Option<impl Into<String>>) -> Self {
        self.media_base_url = url.map(|u| u.into());
        self
    }

    /// 设置单个协议（单协议模式）
    pub fn protocol(mut self, protocol: TransportProtocol) -> Self {
        self.protocol = Some(protocol);
        self.protocols = None; // 单协议模式，清除协议列表
        self
    }

    /// 设置协议列表（协议竞速模式）
    /// 列表顺序就是优先级顺序，前面的优先级更高
    pub fn protocols(mut self, protocols: Vec<TransportProtocol>) -> Self {
        self.protocols = Some(protocols);
        self.protocol = None; // 竞速模式，清除单个协议
        self
    }

    /// 为特定协议设置地址
    pub fn protocol_url(mut self, protocol: TransportProtocol, url: impl Into<String>) -> Self {
        // 优化：预分配容量
        self.protocol_urls
            .get_or_insert_with(|| std::collections::HashMap::with_capacity(4))
            .insert(protocol, url.into());
        self
    }

    /// 批量设置协议地址映射
    ///
    /// # 参数
    /// - `urls`: 协议地址映射（HashMap<TransportProtocol, String>）
    ///
    /// # 示例
    /// ```rust
    /// use std::collections::HashMap;
    /// use flare_core::common::config_types::TransportProtocol;
    /// use flare_im_core_sdk::ClientConfig;
    ///
    /// let mut urls = HashMap::new();
    /// urls.insert(TransportProtocol::QUIC, "quic://im.example.com:8081".to_string());
    /// urls.insert(TransportProtocol::WebSocket, "wss://im.example.com:8080".to_string());
    ///
    /// let config = ClientConfig::builder()
    ///     .server_url("wss://example.com")
    ///     .user_id("u1")
    ///     .device_id("d1")
    ///     .protocol_urls(urls)
    ///     .build().unwrap();
    /// ```
    pub fn protocol_urls(mut self, urls: HashMap<TransportProtocol, String>) -> Self {
        self.protocol_urls = Some(urls);
        self
    }

    /// 设置协议竞速超时时间
    pub fn race_timeout(mut self, timeout: Duration) -> Self {
        self.race_timeout = Some(timeout);
        self
    }

    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    pub fn device_id(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    pub fn device_platform(mut self, platform: DevicePlatform) -> Self {
        self.device_platform = Some(platform);
        self
    }

    pub fn app_version(mut self, version: impl Into<String>) -> Self {
        self.app_version = Some(version.into());
        self
    }

    pub fn connect_timeout(mut self, timeout: u64) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    pub fn heartbeat_interval(mut self, interval: u64) -> Self {
        self.heartbeat_interval = Some(interval);
        self
    }

    pub fn reconnect_interval(mut self, interval: u64) -> Self {
        self.reconnect_interval = Some(interval);
        self
    }

    pub fn max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.max_reconnect_attempts = Some(attempts);
        self
    }

    pub fn auto_reconnect(mut self, enable: bool) -> Self {
        self.auto_reconnect = Some(enable);
        self
    }

    pub fn tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn build(self) -> anyhow::Result<ClientConfig> {
        let config = ClientConfig {
            server_url: self
                .server_url
                .ok_or_else(|| anyhow::anyhow!("server_url is required"))?,
            media_base_url: self.media_base_url,
            protocol: self.protocol,
            protocols: self.protocols,
            protocol_urls: self.protocol_urls,
            race_timeout: self.race_timeout,
            user_id: self
                .user_id
                .ok_or_else(|| anyhow::anyhow!("user_id is required"))?,
            device_id: self.device_id.unwrap_or_else(|| default_device_id()),
            device_platform: self.device_platform.unwrap_or(DevicePlatform::Web),
            app_version: self.app_version,
            connect_timeout: self.connect_timeout.unwrap_or(30),
            heartbeat_interval: self.heartbeat_interval.unwrap_or(30),
            reconnect_interval: self.reconnect_interval.unwrap_or(5),
            max_reconnect_attempts: self.max_reconnect_attempts.unwrap_or(0),
            auto_reconnect: self.auto_reconnect.unwrap_or(true),
            tenant_id: self.tenant_id,
            token: self.token,
            cache_config: self.cache_config.or(Some(CacheConfig::default())),
            message_state_machine_config: self
                .message_state_machine_config
                .or(Some(MessageStateMachineConfig::default())),
            pending_message_queue_config: self
                .pending_message_queue_config
                .or(Some(PendingMessageQueueConfig::default())),
            db_path: self.db_path,
        };

        config.validate()?;
        Ok(config)
    }

    /// 设置缓存配置
    pub fn cache_config(mut self, config: CacheConfig) -> Self {
        self.cache_config = Some(config);
        self
    }

    /// 禁用缓存
    pub fn disable_cache(mut self) -> Self {
        self.cache_config = Some(CacheConfig {
            enabled: false,
            ..Default::default()
        });
        self
    }

    /// 设置消息状态机配置
    pub fn message_state_machine_config(mut self, config: MessageStateMachineConfig) -> Self {
        self.message_state_machine_config = Some(config);
        self
    }

    /// 设置待发送消息队列配置
    pub fn pending_message_queue_config(mut self, config: PendingMessageQueueConfig) -> Self {
        self.pending_message_queue_config = Some(config);
        self
    }

    /// 设置数据库路径（用于 SQLite 存储）
    pub fn db_path(mut self, path: impl Into<String>) -> Self {
        self.db_path = Some(path.into());
        self
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_device_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(target_arch = "wasm32")]
fn default_device_id() -> String {
    let now = Date::now();
    let rnd = Math::floor(Math::random() * 1_000_000.0) as u32;
    format!("web-{}-{}", now as u64, rnd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = ClientConfig::builder()
            .server_url("wss://example.com")
            .media_base_url("https://media.example.com")
            .protocols(vec![TransportProtocol::QUIC, TransportProtocol::WebSocket])
            .race_timeout(Duration::from_secs(5))
            .user_id("user_123")
            .device_id("device_456")
            .build()
            .unwrap();

        assert_eq!(config.server_url, "wss://example.com");
        assert_eq!(config.user_id, "user_123");
        assert!(config.protocols.is_some());
    }

    #[test]
    fn test_config_validation() {
        // 测试空 server_url
        let result = ClientConfig::builder()
            .server_url("")
            .user_id("user_123")
            .device_id("device_456")
            .build();

        assert!(result.is_err());

        // 测试无效 URL
        let result = ClientConfig::builder()
            .server_url("invalid_url")
            .user_id("user_123")
            .device_id("device_456")
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_config_default() {
        let config = ClientConfig::default();
        assert!(!config.server_url.is_empty());
        assert!(!config.device_id.is_empty());
    }

    #[test]
    fn test_protocol_race_mode() {
        let config = ClientConfig::builder()
            .server_url("wss://example.com")
            .protocols(vec![TransportProtocol::QUIC, TransportProtocol::WebSocket])
            .user_id("user_123")
            .device_id("device_456")
            .build()
            .unwrap();

        assert!(config.protocols.is_some());
        assert_eq!(config.protocols.as_ref().unwrap().len(), 2);
        assert!(config.protocol.is_none()); // 竞速模式时，protocol 应该为 None
    }

    #[test]
    fn test_single_protocol_mode() {
        let config = ClientConfig::builder()
            .server_url("wss://example.com")
            .protocol(TransportProtocol::WebSocket)
            .user_id("user_123")
            .device_id("device_456")
            .build()
            .unwrap();

        assert!(config.protocol.is_some());
        assert_eq!(config.protocol.unwrap(), TransportProtocol::WebSocket);
        assert!(config.protocols.is_none()); // 单协议模式时，protocols 应该为 None
    }
}
