//! 客户端构建器
//!
//! 负责构建 FlareClientBuilder，提取配置构建逻辑

use crate::infrastructure::connection::message_listener::SDKMessageListener;
use crate::shared::config::ClientConfig;
#[cfg(not(target_arch = "wasm32"))]
use flare_core::client::builder::flare::FlareClientBuilder;
use flare_core::common::config_types::TransportProtocol;
use std::sync::Arc;

/// 客户端构建器
///
/// 负责从配置构建 FlareClientBuilder
#[cfg(not(target_arch = "wasm32"))]
pub struct ClientBuilder;

#[cfg(not(target_arch = "wasm32"))]
impl ClientBuilder {
    /// 构建 FlareClientBuilder
    ///
    /// 提取配置构建逻辑，包括：
    /// - 中间件配置
    /// - 协议配置
    /// - 设备信息配置
    /// - 用户和 Token 配置
    /// - 连接配置（心跳、超时、重连）
    pub fn build(
        config: &ClientConfig,
        protocols: Vec<TransportProtocol>,
        message_listener: Arc<SDKMessageListener>,
    ) -> anyhow::Result<FlareClientBuilder> {
        use flare_core::common::config_types::HeartbeatConfig;
        use flare_core::common::message::{
            ArcMessageMiddleware, LogLevel, LoggingMiddleware, MetricsMiddleware,
        };

        let mut builder = FlareClientBuilder::new(config.server_url.clone());

        // 设置消息监听器
        builder = builder.with_listener(message_listener);

        // 添加中间件
        let logging_middleware =
            Arc::new(LoggingMiddleware::new("SDKClientLogging").with_level(LogLevel::Info))
                as ArcMessageMiddleware;
        builder = builder.with_middleware(logging_middleware);

        let metrics_middleware =
            Arc::new(MetricsMiddleware::new("SDKClientMetrics")) as ArcMessageMiddleware;
        builder = builder.with_middleware(metrics_middleware);

        // 协议配置
        builder = builder.with_protocol_race(protocols.clone());

        if let Some(ref protocol_urls) = config.protocol_urls {
            for (protocol, url) in protocol_urls {
                builder = builder.with_protocol_url(*protocol, url.clone());
            }
        }

        // 设备信息配置
        let device_info = Self::build_device_info(config)?;
        builder = builder.with_device_info(device_info);

        // 用户 ID 和 Token 配置
        if !config.user_id.is_empty() {
            builder = builder.with_user_id(config.user_id.clone());
        }

        if let Some(ref token) = config.token {
            builder = builder.with_token(token.clone());
        }

        // 连接配置
        let heartbeat_config = HeartbeatConfig::default()
            .with_interval(std::time::Duration::from_secs(config.heartbeat_interval))
            .with_timeout(std::time::Duration::from_secs(
                config.heartbeat_interval * 3,
            ));
        builder = builder.with_heartbeat(heartbeat_config);

        builder =
            builder.with_connect_timeout(std::time::Duration::from_secs(config.connect_timeout));

        let race_timeout = config
            .race_timeout
            .unwrap_or(std::time::Duration::from_secs(config.connect_timeout));
        builder = builder.with_race_timeout(race_timeout);

        builder = builder
            .with_reconnect_interval(std::time::Duration::from_secs(config.reconnect_interval));

        if config.max_reconnect_attempts > 0 {
            builder = builder.with_max_reconnect_attempts(Some(config.max_reconnect_attempts));
        }

        Ok(builder)
    }

    /// 构建设备信息
    ///
    /// 从配置中构建完整的设备信息，包括平台、型号、版本等
    fn build_device_info(
        config: &ClientConfig,
    ) -> anyhow::Result<flare_core::common::device::DeviceInfo> {
        use flare_core::common::device::{
            DeviceInfo as FlareDeviceInfo, DevicePlatform as FlareDevicePlatform,
        };

        let platform = match config.device_platform {
            crate::shared::config::DevicePlatform::Web => FlareDevicePlatform::Web,
            crate::shared::config::DevicePlatform::Android => FlareDevicePlatform::Android,
            crate::shared::config::DevicePlatform::IOS => FlareDevicePlatform::IOS,
            crate::shared::config::DevicePlatform::HarmonyOS => FlareDevicePlatform::HarmonyOS,
            crate::shared::config::DevicePlatform::Desktop => FlareDevicePlatform::PC,
        };

        let mut device_info = FlareDeviceInfo::new(config.device_id.clone(), platform.clone());

        // 设置 model（使用平台名称作为标识）
        device_info = device_info.with_model(platform.as_str().to_string());

        // 设置应用版本
        let app_version = config
            .app_version
            .clone()
            .unwrap_or_else(|| "1.0.0".to_string());
        device_info = device_info.with_app_version(app_version);

        // 设置系统版本（仅用于记录，不作为平台判定标准）
        let system_version = match &platform {
            FlareDevicePlatform::PC => "macOS/Linux/Windows".to_string(),
            FlareDevicePlatform::Android => "Android".to_string(),
            FlareDevicePlatform::IOS => "iOS".to_string(),
            FlareDevicePlatform::Web => "Web Browser".to_string(),
            FlareDevicePlatform::H5 => "Mobile Browser".to_string(),
            FlareDevicePlatform::HarmonyOS => "HarmonyOS".to_string(),
            FlareDevicePlatform::Other(_) => "Unknown".to_string(),
        };
        device_info = device_info.with_system_version(system_version);

        Ok(device_info)
    }
}
