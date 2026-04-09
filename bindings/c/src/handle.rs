//! 句柄管理模块
//!
//! 提供句柄类型定义、全局句柄表、句柄生命周期管理

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use flare_im_core_sdk::client::IMClient;

use crate::error::FlareErrorCode;

/// 全局句柄 ID 生成器
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// 全局订阅 ID 生成器
static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);

/// SDK 实例句柄（C 侧可见）
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlareImHandle {
    pub id: u64,
}

impl Default for FlareImHandle {
    fn default() -> Self {
        Self { id: 0 }
    }
}

/// 事件订阅句柄（C 侧可见）
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlareEventSubscription {
    pub id: u64,
}

impl Default for FlareEventSubscription {
    fn default() -> Self {
        Self { id: 0 }
    }
}

/// 事件订阅内部状态
pub struct EventSubscriptionInner {
    pub id: u64,
    pub cancel_tx: tokio::sync::oneshot::Sender<()>,
}

/// SDK 实例内部状态
pub struct SdkInstance {
    /// IM 客户端
    pub client: IMClient,
    /// Tokio runtime handle
    pub runtime: tokio::runtime::Handle,
    /// 事件订阅列表
    pub event_subscriptions: RwLock<Vec<Arc<EventSubscriptionInner>>>,
}

/// 全局句柄表（懒加载）
lazy_static::lazy_static! {
    static ref HANDLE_TABLE: RwLock<HashMap<u64, Arc<SdkInstance>>> = RwLock::new(HashMap::new());
}

/// 生成新的句柄 ID
pub fn next_handle_id() -> u64 {
    NEXT_HANDLE_ID.fetch_add(1, Ordering::SeqCst)
}

/// 生成新的订阅 ID
pub fn next_subscription_id() -> u64 {
    NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::SeqCst)
}

/// 注册 SDK 实例到句柄表
pub fn register_instance(id: u64, instance: Arc<SdkInstance>) -> Result<(), FlareErrorCode> {
    HANDLE_TABLE
        .write()
        .map_err(|_| FlareErrorCode::InternalError)?
        .insert(id, instance);
    Ok(())
}

/// 从句柄获取实例
pub fn get_instance(handle: FlareImHandle) -> Result<Arc<SdkInstance>, FlareErrorCode> {
    if handle.id == 0 {
        return Err(FlareErrorCode::InvalidHandle);
    }
    let table = HANDLE_TABLE.read().map_err(|_| FlareErrorCode::InternalError)?;
    table.get(&handle.id).cloned().ok_or(FlareErrorCode::InvalidHandle)
}

/// 从句柄表移除实例
pub fn remove_instance(handle: FlareImHandle) -> Result<Arc<SdkInstance>, FlareErrorCode> {
    if handle.id == 0 {
        return Err(FlareErrorCode::InvalidHandle);
    }
    HANDLE_TABLE
        .write()
        .map_err(|_| FlareErrorCode::InternalError)?
        .remove(&handle.id)
        .ok_or(FlareErrorCode::InvalidHandle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_default() {
        let handle = FlareImHandle::default();
        assert_eq!(handle.id, 0);
    }

    #[test]
    fn test_subscription_default() {
        let sub = FlareEventSubscription::default();
        assert_eq!(sub.id, 0);
    }

    #[test]
    fn test_next_handle_id() {
        let id1 = next_handle_id();
        let id2 = next_handle_id();
        assert!(id2 > id1);
        assert!(id1 > 0);
    }

    #[test]
    fn test_next_subscription_id() {
        let id1 = next_subscription_id();
        let id2 = next_subscription_id();
        assert!(id2 > id1);
        assert!(id1 > 0);
    }

    #[test]
    fn test_get_instance_invalid() {
        let handle = FlareImHandle { id: 0 };
        let result = get_instance(handle);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), FlareErrorCode::InvalidHandle);
    }

    #[test]
    fn test_get_instance_nonexistent() {
        let handle = FlareImHandle { id: 999999 };
        let result = get_instance(handle);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), FlareErrorCode::InvalidHandle);
    }
}
