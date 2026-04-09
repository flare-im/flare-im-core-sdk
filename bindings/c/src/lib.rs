//! Flare IM Core SDK - C FFI Bindings
//!
//! 提供统一的 C ABI 绑定层，作为 Rust SDK 与各平台之间的唯一桥梁。
//!
//! # 架构原则
//!
//! - **Rust 只维护一套核心业务** - 所有业务逻辑在 `flare-im-core-sdk` 中实现
//! - **对外统一导出 C ABI** - 本模块是唯一的 FFI 层，所有平台共用
//! - **平台极薄适配层** - 各平台适配层代码量控制在 200-400 LOC
//! - **接口统一设计** - 采用"句柄 + 错误码 + JSON/bytes + 回调"统一模式
//!
//! # 模块结构
//!
//! - `error`: 错误码定义与映射
//! - `handle`: 句柄管理（SDK 实例、订阅）
//! - `callback`: 回调管理（存储、调度）
//! - `lifecycle`: 生命周期 API（init/login/logout）
//! - `message`: 消息 API（构建、发送、查询、操作）
//! - `conversation`: 会话 API（列表、操作）
//! - `media`: 媒体 API（上传、下载、缓存）
//! - `event`: 事件订阅 API
//! - `json`: JSON 辅助工具
//! - `string`: 字符串内存管理

// 模块声明
pub mod callback;
pub mod conversation;
pub mod error;
pub mod event;
pub mod handle;
pub mod json;
pub mod lifecycle;
pub mod media;
pub mod message;
pub mod string;

// 重新导出公开类型
pub use error::FlareErrorCode;
pub use handle::{FlareEventSubscription, FlareImHandle};

/// FFI 边界 panic 捕获
///
/// 所有 `extern "C"` 函数必须使用此函数捕获 panic，不可让异常跨越 FFI。
///
/// # Safety
/// 此函数确保 FFI 边界的安全，防止 panic 跨越 FFI 边界。
pub fn catch_panic<F, R>(f: F) -> R
where
    F: FnOnce() -> Result<R, FlareErrorCode> + std::panic::UnwindSafe,
    R: Default,
{
    match std::panic::catch_unwind(f) {
        Ok(Ok(result)) => result,
        Ok(Err(code)) => {
            tracing::error!("FFI error: {:?}", code);
            R::default()
        }
        Err(e) => {
            tracing::error!("FFI panic: {:?}", e);
            R::default()
        }
    }
}

/// 初始化日志
///
/// 应在首次使用 SDK 前调用，确保日志系统已初始化。
#[unsafe(no_mangle)]
pub extern "C" fn flare_im_init_logging() {
    let _ = tracing_subscriber::fmt::try_init();
}
