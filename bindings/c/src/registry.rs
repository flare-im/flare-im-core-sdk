//! 句柄注册表 - 使用 DashMap 管理所有对象
//!
//! 所有 Rust 对象通过句柄管理,禁止跨 ABI 传递指针

use dashmap::DashMap;
use flare_im_core_sdk::client::IMClient;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error_convert::FLARE_ERR_INVALID_HANDLE;
use crate::types::FlareHandle;

/// SDK 实例包装
pub struct SdkInstance {
    pub client: IMClient,
    pub runtime: tokio::runtime::Handle,
}

/// 全局句柄 ID 生成器
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

// 全局句柄 → SdkInstance（文档见模块级说明；`lazy_static!` 上无法用 `///` 挂 rustdoc）
lazy_static::lazy_static! {
    static ref HANDLE_REGISTRY: DashMap<u64, Arc<SdkInstance>> = DashMap::new();
}

/// 生成新句柄
#[inline]
pub fn alloc_handle() -> FlareHandle {
    NEXT_HANDLE.fetch_add(1, Ordering::SeqCst)
}

/// 注册实例
#[inline]
pub fn register_instance(instance: Arc<SdkInstance>) -> FlareHandle {
    let handle = alloc_handle();
    HANDLE_REGISTRY.insert(handle, instance);
    handle
}

/// 获取实例 (增加引用计数)
#[inline]
pub fn retain_instance(handle: FlareHandle) -> Option<Arc<SdkInstance>> {
    HANDLE_REGISTRY.get(&handle).map(|v| Arc::clone(v.value()))
}

/// 在 FFI 闭包内解析句柄；无效时返回 `FLARE_ERR_INVALID_HANDLE`（单一出口，便于审计）。
#[inline]
pub fn require_instance(handle: FlareHandle) -> Result<Arc<SdkInstance>, i32> {
    retain_instance(handle).ok_or(FLARE_ERR_INVALID_HANDLE)
}

/// 释放实例 (减少引用计数)
#[inline]
pub fn release_instance(handle: FlareHandle) {
    HANDLE_REGISTRY.remove(&handle);
}

/// 释放所有实例（主要用于宿主热重启后的兜底重置）。
#[inline]
pub fn release_all_instances() {
    let handles: Vec<FlareHandle> = HANDLE_REGISTRY.iter().map(|e| *e.key()).collect();
    for h in handles {
        HANDLE_REGISTRY.remove(&h);
    }
}
