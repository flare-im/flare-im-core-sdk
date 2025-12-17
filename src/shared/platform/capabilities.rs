//! 平台能力定义
//!
//! 定义不同平台支持的能力

/// 存储类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    /// SQLite（桌面端和移动端）
    SQLite,

    /// IndexedDB（Web 端）
    IndexedDB,
}

/// 网络类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    /// 仅支持 WebSocket
    WebSocket,

    /// 仅支持 QUIC
    QUIC,

    /// 同时支持 WebSocket 和 QUIC
    Both,
}

/// 平台能力
#[derive(Debug, Clone)]
pub struct PlatformCapabilities {
    /// 存储类型
    pub storage: StorageType,

    /// 网络类型
    pub network: NetworkType,

    /// 是否支持后台运行
    pub background: bool,

    /// 是否支持推送通知
    pub push: bool,

    /// 内存限制（MB）
    pub memory_limit_mb: Option<usize>,
}

impl PlatformCapabilities {
    /// 创建 Web 平台能力
    pub fn web() -> Self {
        Self {
            storage: StorageType::IndexedDB,
            network: NetworkType::WebSocket,
            background: false,
            push: false,
            memory_limit_mb: Some(50),
        }
    }

    /// 创建桌面平台能力
    pub fn desktop() -> Self {
        Self {
            storage: StorageType::SQLite,
            network: NetworkType::Both,
            background: true,
            push: true,
            memory_limit_mb: None,
        }
    }

    /// 创建移动平台能力
    pub fn mobile() -> Self {
        Self {
            storage: StorageType::SQLite,
            network: NetworkType::Both,
            background: true,
            push: true,
            memory_limit_mb: Some(200),
        }
    }
}
