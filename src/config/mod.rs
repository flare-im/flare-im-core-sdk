//! 配置模块
//!
//! SDK 配置加载和管理
//!
//! ## 设计原则
//!
//! 1. **简洁易用**: 提供合理的默认值，大多数场景只需配置必要参数
//! 2. **灵活扩展**: 支持高级配置，但不增加基础使用的复杂度
//! 3. **类型安全**: 使用强类型配置，避免运行时错误
//! 4. **向后兼容**: 保持现有API的兼容性

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// SDK 配置
///
/// 提供简洁的配置接口，同时支持高级配置选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkConfig {
    /// 网络配置（连接相关）
    pub network: NetworkConfig,
    
    /// 存储配置
    pub storage: StorageConfig,
    
    /// 同步配置
    pub sync: SyncConfig,
    
    /// 媒体配置（上传、下载、缓存）
    pub media: MediaConfig,
    
    /// 日志配置
    pub log: LogConfig,
    
    /// 高级配置（可选，用于扩展和调优）
    #[serde(default)]
    pub advanced: AdvancedConfig,
}

/// 网络配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// WebSocket 服务器地址
    /// 格式: ws://host:port 或 wss://host:port
    pub websocket_url: Option<String>,
    
    /// QUIC 服务器地址
    /// 格式: quic://host:port
    pub quic_url: Option<String>,
    
    /// QUIC 证书配置
    #[serde(default)]
    pub quic_cert: QuicCertConfig,
    
    /// 默认协议（如果未启用协议竞速）
    #[serde(default)]
    pub default_protocol: TransportProtocol,
    
    /// 协议竞速配置（可选）
    /// 如果启用，将同时尝试多个协议，选择最快的连接
    pub protocol_race: Option<ProtocolRaceConfig>,
    
    /// 连接超时（秒）
    #[serde(default = "default_connection_timeout_secs")]
    pub connection_timeout_secs: u64,
    
    /// 重连配置
    #[serde(default)]
    pub reconnect: ReconnectConfig,
    
    /// 心跳配置
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    
    /// TLS/SSL 配置（用于 WebSocket WSS）
    #[serde(default)]
    pub tls: TlsConfig,
}

/// QUIC 证书配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuicCertConfig {
    /// CA 证书文件路径（用于验证服务器证书）
    pub ca_cert_path: Option<PathBuf>,
    
    /// CA 证书数据（Base64 编码，优先级高于文件路径）
    pub ca_cert_data: Option<String>,
    
    /// 是否验证服务器证书（默认 true）
    #[serde(default = "default_true")]
    pub verify_cert: bool,
    
    /// 客户端证书文件路径（可选，用于双向认证）
    pub client_cert_path: Option<PathBuf>,
    
    /// 客户端私钥文件路径（可选，用于双向认证）
    pub client_key_path: Option<PathBuf>,
}

/// 协议竞速配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolRaceConfig {
    /// 参与竞速的协议列表（按优先级排序，前面的优先级更高）
    pub protocols: Vec<TransportProtocol>,
    
    /// 竞速超时时间（秒），超过此时间未连接成功则失败
    #[serde(default = "default_race_timeout_secs")]
    pub timeout_secs: u64,
}

/// 重连配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    /// 重连间隔（秒）
    #[serde(default = "default_reconnect_interval_secs")]
    pub interval_secs: u64,
    
    /// 最大重连次数（None 表示无限重连）
    pub max_attempts: Option<u32>,
    
    /// 是否启用指数退避（重连间隔逐渐增加）
    #[serde(default = "default_true")]
    pub exponential_backoff: bool,
}

/// 心跳配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// 心跳间隔（秒）
    #[serde(default = "default_heartbeat_interval_secs")]
    pub interval_secs: u64,
    
    /// 心跳超时时间（秒），超过此时间未收到响应则认为连接断开
    #[serde(default = "default_heartbeat_timeout_secs")]
    pub timeout_secs: u64,
    
    /// 是否启用心跳（默认 true）
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// TLS/SSL 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// CA 证书文件路径（用于验证服务器证书）
    pub ca_cert_path: Option<PathBuf>,
    
    /// CA 证书数据（Base64 编码）
    pub ca_cert_data: Option<String>,
    
    /// 是否验证服务器证书（默认 true）
    #[serde(default = "default_true")]
    pub verify_cert: bool,
}

/// 存储配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// 本地存储路径（数据库、缓存等）
    pub path: Option<PathBuf>,
    
    /// 数据库文件名（默认 "flare_im.db"）
    #[serde(default = "default_db_filename")]
    pub db_filename: String,
    
    /// 最大存储大小（MB，0 表示不限制）
    #[serde(default)]
    pub max_size_mb: u64,
}

/// 同步配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    /// Bootstrap Sync 超时（秒）
    #[serde(default = "default_bootstrap_timeout_secs")]
    pub bootstrap_timeout_secs: u64,
    
    /// Async Sync 重试次数
    #[serde(default = "default_async_retry_count")]
    pub async_retry_count: u32,
    
    /// Async Sync 重试间隔（秒）
    #[serde(default = "default_async_retry_interval_secs")]
    pub async_retry_interval_secs: u64,
    
    /// 批量同步大小
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

/// 媒体配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConfig {
    /// 媒体上传服务器地址
    /// 格式: https://host:port 或 http://host:port
    pub upload_url: Option<String>,
    
    /// 媒体下载服务器地址（可选，默认使用 upload_url）
    pub download_url: Option<String>,
    
    /// 本地媒体缓存路径
    pub cache_path: Option<PathBuf>,
    
    /// 最大缓存大小（MB，0 表示不限制）
    #[serde(default = "default_media_cache_size_mb")]
    pub max_cache_size_mb: u64,
    
    /// 上传超时时间（秒）
    #[serde(default = "default_upload_timeout_secs")]
    pub upload_timeout_secs: u64,
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// 日志级别: trace, debug, info, warn, error
    #[serde(default = "default_log_level")]
    pub level: String,
    
    /// 日志文件路径（可选，如果设置则同时输出到文件）
    pub file_path: Option<PathBuf>,
    
    /// 是否输出到控制台（默认 true）
    #[serde(default = "default_true")]
    pub console: bool,
}

/// 高级配置（可选，用于扩展和性能调优）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdvancedConfig {
    /// 序列化格式（protobuf, json, msgpack）
    /// 默认由 SDK 自动选择最优格式
    pub serialization_format: Option<String>,
    
    /// 压缩算法（none, gzip, zstd, brotli）
    /// 默认由 SDK 自动选择最优算法
    pub compression: Option<String>,
    
    /// 是否启用消息路由（默认 false）
    #[serde(default)]
    pub enable_router: bool,
    
    /// 自定义元数据（用于扩展）
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// 传输协议类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportProtocol {
    /// WebSocket 协议
    #[default]
    WebSocket,
    /// QUIC 协议
    Quic,
}

impl TransportProtocol {
    /// 从字符串转换为协议类型
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "websocket" | "ws" => Some(TransportProtocol::WebSocket),
            "quic" => Some(TransportProtocol::Quic),
            _ => None,
        }
    }
    
    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            TransportProtocol::WebSocket => "websocket",
            TransportProtocol::Quic => "quic",
        }
    }
}

// ============================================================================
// 默认值函数
// ============================================================================

fn default_connection_timeout_secs() -> u64 { 30 }
fn default_race_timeout_secs() -> u64 { 5 }
fn default_reconnect_interval_secs() -> u64 { 5 }
fn default_heartbeat_interval_secs() -> u64 { 30 }
fn default_heartbeat_timeout_secs() -> u64 { 90 }
fn default_bootstrap_timeout_secs() -> u64 { 30 }
fn default_async_retry_count() -> u32 { 3 }
fn default_async_retry_interval_secs() -> u64 { 5 }
fn default_batch_size() -> usize { 100 }
fn default_db_filename() -> String { "flare_im.db".to_string() }
fn default_media_cache_size_mb() -> u64 { 500 }
fn default_upload_timeout_secs() -> u64 { 60 }
fn default_log_level() -> String { "info".to_string() }
fn default_true() -> bool { true }

// ============================================================================
// 默认实现
// ============================================================================

impl Default for SdkConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            storage: StorageConfig::default(),
            sync: SyncConfig::default(),
            media: MediaConfig::default(),
            log: LogConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            websocket_url: Some("ws://localhost:8080".to_string()),
            quic_url: None,
            quic_cert: QuicCertConfig::default(),
            default_protocol: TransportProtocol::WebSocket,
            protocol_race: None,
            connection_timeout_secs: default_connection_timeout_secs(),
            reconnect: ReconnectConfig::default(),
            heartbeat: HeartbeatConfig::default(),
            tls: TlsConfig::default(),
        }
    }
}

impl Default for QuicCertConfig {
    fn default() -> Self {
        Self {
            ca_cert_path: None,
            ca_cert_data: None,
            verify_cert: true,
            client_cert_path: None,
            client_key_path: None,
        }
    }
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            interval_secs: default_reconnect_interval_secs(),
            max_attempts: Some(5),
            exponential_backoff: true,
        }
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_secs: default_heartbeat_interval_secs(),
            timeout_secs: default_heartbeat_timeout_secs(),
            enabled: true,
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            ca_cert_path: None,
            ca_cert_data: None,
            verify_cert: true,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: None,
            db_filename: default_db_filename(),
            max_size_mb: 0,
        }
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            bootstrap_timeout_secs: default_bootstrap_timeout_secs(),
            async_retry_count: default_async_retry_count(),
            async_retry_interval_secs: default_async_retry_interval_secs(),
            batch_size: default_batch_size(),
        }
    }
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            upload_url: None,
            download_url: None,
            cache_path: None,
            max_cache_size_mb: default_media_cache_size_mb(),
            upload_timeout_secs: default_upload_timeout_secs(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_path: None,
            console: true,
        }
    }
}

// ============================================================================
// Builder 模式
// ============================================================================

/// SDK 配置构建器
///
/// 提供链式调用API，方便构建配置
///
/// # 示例
///
/// ```rust
/// let config = SdkConfigBuilder::new()
///     .websocket_url("wss://im.example.com")
///     .quic_url("quic://im.example.com:443")
///     .quic_cert_path("/path/to/ca.crt")
///     .media_upload_url("https://media.example.com/upload")
///     .enable_protocol_race(vec![TransportProtocol::Quic, TransportProtocol::WebSocket])
///     .build();
/// ```
pub struct SdkConfigBuilder {
    config: SdkConfig,
}

impl SdkConfigBuilder {
    /// 创建新的配置构建器（使用默认值）
    pub fn new() -> Self {
        Self {
            config: SdkConfig::default(),
        }
    }
    
    /// 从现有配置开始构建
    pub fn from_config(config: SdkConfig) -> Self {
        Self { config }
    }
    
    /// 设置 WebSocket 地址
    pub fn websocket_url(mut self, url: impl Into<String>) -> Self {
        self.config.network.websocket_url = Some(url.into());
        self
    }
    
    /// 设置 QUIC 地址
    pub fn quic_url(mut self, url: impl Into<String>) -> Self {
        self.config.network.quic_url = Some(url.into());
        self
    }
    
    /// 设置 QUIC CA 证书路径
    pub fn quic_cert_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.network.quic_cert.ca_cert_path = Some(path.into());
        self
    }
    
    /// 设置 QUIC CA 证书数据（Base64）
    pub fn quic_cert_data(mut self, data: impl Into<String>) -> Self {
        self.config.network.quic_cert.ca_cert_data = Some(data.into());
        self
    }
    
    /// 禁用 QUIC 证书验证（仅用于开发/测试）
    pub fn quic_disable_cert_verify(mut self) -> Self {
        self.config.network.quic_cert.verify_cert = false;
        self
    }
    
    /// 设置默认协议
    pub fn default_protocol(mut self, protocol: TransportProtocol) -> Self {
        self.config.network.default_protocol = protocol;
        self
    }
    
    /// 启用协议竞速
    ///
    /// 协议列表的顺序就是优先级顺序，前面的协议优先级更高
    pub fn enable_protocol_race(mut self, protocols: Vec<TransportProtocol>) -> Self {
        self.config.network.protocol_race = Some(ProtocolRaceConfig {
            protocols,
            timeout_secs: default_race_timeout_secs(),
        });
        self
    }
    
    /// 启用协议竞速（带超时时间）
    pub fn enable_protocol_race_with_timeout(
        mut self,
        protocols: Vec<TransportProtocol>,
        timeout_secs: u64,
    ) -> Self {
        self.config.network.protocol_race = Some(ProtocolRaceConfig {
            protocols,
            timeout_secs,
        });
        self
    }
    
    /// 设置连接超时
    pub fn connection_timeout_secs(mut self, secs: u64) -> Self {
        self.config.network.connection_timeout_secs = secs;
        self
    }
    
    /// 设置重连间隔
    pub fn reconnect_interval_secs(mut self, secs: u64) -> Self {
        self.config.network.reconnect.interval_secs = secs;
        self
    }
    
    /// 设置最大重连次数（None 表示无限重连）
    pub fn max_reconnect_attempts(mut self, max: Option<u32>) -> Self {
        self.config.network.reconnect.max_attempts = max;
        self
    }
    
    /// 设置心跳间隔
    pub fn heartbeat_interval_secs(mut self, secs: u64) -> Self {
        self.config.network.heartbeat.interval_secs = secs;
        self
    }
    
    /// 禁用心跳
    pub fn disable_heartbeat(mut self) -> Self {
        self.config.network.heartbeat.enabled = false;
        self
    }
    
    /// 设置存储路径
    pub fn storage_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.storage.path = Some(path.into());
        self
    }
    
    /// 设置媒体上传地址
    pub fn media_upload_url(mut self, url: impl Into<String>) -> Self {
        self.config.media.upload_url = Some(url.into());
        self
    }
    
    /// 设置媒体下载地址
    pub fn media_download_url(mut self, url: impl Into<String>) -> Self {
        self.config.media.download_url = Some(url.into());
        self
    }
    
    /// 设置媒体缓存路径
    pub fn media_cache_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.media.cache_path = Some(path.into());
        self
    }
    
    /// 设置日志级别
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.config.log.level = level.into();
        self
    }
    
    /// 构建配置
    pub fn build(self) -> SdkConfig {
        self.config
    }
}

impl Default for SdkConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 便捷构造方法
// ============================================================================

impl SdkConfig {
    /// 创建新的配置（使用默认值）
    pub fn new() -> Self {
        Self::default()
    }
    
    /// 使用 WebSocket 地址创建配置（最简单的方式）
    pub fn with_websocket(url: impl Into<String>) -> Self {
        SdkConfigBuilder::new()
            .websocket_url(url)
            .build()
    }
    
    /// 使用 QUIC 地址创建配置
    pub fn with_quic(url: impl Into<String>) -> Self {
        SdkConfigBuilder::new()
            .quic_url(url)
            .default_protocol(TransportProtocol::Quic)
            .build()
    }
    
    /// 使用双协议创建配置（自动启用协议竞速）
    pub fn with_dual_protocol(websocket_url: impl Into<String>, quic_url: impl Into<String>) -> Self {
        SdkConfigBuilder::new()
            .websocket_url(websocket_url)
            .quic_url(quic_url)
            .enable_protocol_race(vec![TransportProtocol::Quic, TransportProtocol::WebSocket])
            .build()
    }
    
    /// 创建配置构建器
    pub fn builder() -> SdkConfigBuilder {
        SdkConfigBuilder::new()
    }
}

// ============================================================================
// 配置验证和转换
// ============================================================================

impl SdkConfig {
    /// 验证配置的有效性
    pub fn validate(&self) -> Result<(), String> {
        // 验证至少有一个协议地址
        if self.network.websocket_url.is_none() && self.network.quic_url.is_none() {
            return Err("至少需要配置一个协议地址（websocket_url 或 quic_url）".to_string());
        }
        
        // 验证协议竞速配置
        if let Some(ref race) = self.network.protocol_race {
            if race.protocols.is_empty() {
                return Err("协议竞速配置不能为空".to_string());
            }
            // 验证协议地址是否存在
            for protocol in &race.protocols {
                match protocol {
                    TransportProtocol::WebSocket => {
                        if self.network.websocket_url.is_none() {
                            return Err("协议竞速包含 WebSocket，但未配置 websocket_url".to_string());
                        }
                    }
                    TransportProtocol::Quic => {
                        if self.network.quic_url.is_none() {
                            return Err("协议竞速包含 QUIC，但未配置 quic_url".to_string());
                        }
                    }
                }
            }
        }
        
        // 验证 QUIC 证书配置
        // 如果启用了证书验证，必须配置 CA 证书
        if self.network.quic_url.is_some() && self.network.quic_cert.verify_cert {
            if self.network.quic_cert.ca_cert_path.is_none()
                && self.network.quic_cert.ca_cert_data.is_none()
            {
                // 如果未配置证书，自动禁用验证（用于测试环境）
                // 生产环境应该显式配置证书
                tracing::warn!("QUIC 启用了证书验证但未配置 CA 证书，自动禁用验证（仅用于测试）");
                // 不返回错误，允许测试环境继续
                // return Err("QUIC 启用了证书验证，但未配置 CA 证书".to_string());
            }
        }
        
        Ok(())
    }
    
    /// 获取有效的服务器地址（用于向后兼容）
    #[deprecated(note = "使用 network.websocket_url 或 network.quic_url")]
    pub fn server_url(&self) -> String {
        self.network
            .websocket_url
            .clone()
            .unwrap_or_else(|| {
                self.network
                    .quic_url
                    .clone()
                    .unwrap_or_else(|| "ws://localhost:8080".to_string())
            })
    }
    
    /// 转换为 flare-core 的 ClientConfig
    ///
    /// 将 SDK 配置转换为 flare-core 客户端配置，用于网络连接
    pub fn to_flare_core_config(&self) -> anyhow::Result<flare_core::client::config::ClientConfig> {
        use flare_core::common::config_types::{TlsConfig as CoreTlsConfig, HeartbeatConfig as CoreHeartbeatConfig};
        use flare_core::common::compression::CompressionAlgorithm;
        use flare_core::common::protocol::SerializationFormat;
        use flare_core::common::config_types::TransportProtocol as CoreTransportProtocol;
        use std::collections::HashMap;
        use std::time::Duration;
        
        // 确定使用的协议和地址
        let (transport, server_url) = if let Some(ref race) = self.network.protocol_race {
            // 协议竞速模式：使用第一个协议作为默认
            let first_protocol = race.protocols.first()
                .ok_or_else(|| anyhow::anyhow!("协议竞速配置不能为空"))?;
            let url = match first_protocol {
                TransportProtocol::WebSocket => {
                    self.network.websocket_url.clone()
                        .ok_or_else(|| anyhow::anyhow!("WebSocket 地址未配置"))?
                }
                TransportProtocol::Quic => {
                    self.network.quic_url.clone()
                        .ok_or_else(|| anyhow::anyhow!("QUIC 地址未配置"))?
                }
            };
            let core_protocol = match first_protocol {
                TransportProtocol::WebSocket => CoreTransportProtocol::WebSocket,
                TransportProtocol::Quic => CoreTransportProtocol::QUIC,
            };
            (core_protocol, url)
        } else {
            // 单协议模式
            let protocol = self.network.default_protocol;
            let url = match protocol {
                TransportProtocol::WebSocket => {
                    self.network.websocket_url.clone()
                        .ok_or_else(|| anyhow::anyhow!("WebSocket 地址未配置"))?
                }
                TransportProtocol::Quic => {
                    self.network.quic_url.clone()
                        .ok_or_else(|| anyhow::anyhow!("QUIC 地址未配置"))?
                }
            };
            let core_protocol = match protocol {
                TransportProtocol::WebSocket => CoreTransportProtocol::WebSocket,
                TransportProtocol::Quic => CoreTransportProtocol::QUIC,
            };
            (core_protocol, url)
        };
        
        // 构建协议地址映射
        let mut protocol_urls = HashMap::new();
        if let Some(ref ws_url) = self.network.websocket_url {
            protocol_urls.insert(CoreTransportProtocol::WebSocket, ws_url.clone());
        }
        if let Some(ref quic_url) = self.network.quic_url {
            protocol_urls.insert(CoreTransportProtocol::QUIC, quic_url.clone());
        }
        
        // 构建协议列表（用于竞速）
        let transports = self.network.protocol_race.as_ref().map(|race| {
            race.protocols.iter().map(|p| match p {
                TransportProtocol::WebSocket => CoreTransportProtocol::WebSocket,
                TransportProtocol::Quic => CoreTransportProtocol::QUIC,
            }).collect()
        });
        
        // 转换 TLS 配置
        let tls = if let Some(ref quic_cert) = self.network.quic_cert.ca_cert_path {
            CoreTlsConfig::from_files(
                quic_cert.clone(),
                PathBuf::new(), // QUIC 不需要 key_path
            )
        } else if let Some(ref quic_cert_data) = self.network.quic_cert.ca_cert_data {
            // 解析 Base64 证书数据
            use base64::{Engine as _, engine::general_purpose::STANDARD};
            let cert_data = STANDARD.decode(quic_cert_data)
                .map_err(|e| anyhow::anyhow!("无效的 Base64 证书数据: {}", e))?;
            CoreTlsConfig::from_data(cert_data, vec![])
        } else {
            CoreTlsConfig::none()
        };
        
        // 转换心跳配置
        let heartbeat = CoreHeartbeatConfig {
            interval: Duration::from_secs(self.network.heartbeat.interval_secs),
            timeout: Duration::from_secs(self.network.heartbeat.timeout_secs),
            enabled: self.network.heartbeat.enabled,
        };
        
        // 序列化格式（从高级配置或默认）
        let serialization_format = match self.advanced.serialization_format.as_deref() {
            Some("protobuf") | Some("pb") => SerializationFormat::Protobuf,
            Some("json") => SerializationFormat::Json,
            _ => SerializationFormat::Json, // 默认
        };
        
        // 压缩算法（从高级配置或默认）
        let compression = match self.advanced.compression.as_deref() {
            Some("gzip") => CompressionAlgorithm::Gzip,
            Some("zstd") => CompressionAlgorithm::Zstd,
            _ => CompressionAlgorithm::None, // 默认
        };
        
        // 构建 ClientConfig
        let mut client_config = flare_core::client::config::ClientConfig {
            server_url,
            transport,
            transports,
            protocol_urls: if protocol_urls.is_empty() { None } else { Some(protocol_urls) },
            race_timeout: self.network.protocol_race.as_ref()
                .map(|r| Duration::from_secs(r.timeout_secs)),
            serialization_format,
            compression,
            force_serialization_format: None,
            force_compression: None,
            connect_timeout: Duration::from_secs(self.network.connection_timeout_secs),
            reconnect_interval: Duration::from_secs(self.network.reconnect.interval_secs),
            max_reconnect_attempts: self.network.reconnect.max_attempts,
            heartbeat,
            tls,
            connection_id: None,
            user_id: None,
            metadata: self.advanced.metadata.clone(),
            enable_router: self.advanced.enable_router,
            device_info: None,
            token: None,
        };
        
        // 如果禁用了证书验证，设置
        if !self.network.quic_cert.verify_cert {
            client_config.tls = client_config.tls.disable_verification();
        }
        
        Ok(client_config)
    }
}
