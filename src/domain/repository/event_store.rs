//! 事件存储接口
//!
//! 用于事件溯源（Event Sourcing）模式，存储领域事件。
//! 事件是不可变的，只能追加，不能修改或删除。

use async_trait::async_trait;
use crate::domain::event::DomainEvent;

/// 事件存储接口
///
/// 用于事件溯源（Event Sourcing）模式，存储领域事件。
/// 事件是不可变的，只能追加，不能修改或删除。
///
/// ## 实现要求
///
/// - 必须保证事件的顺序性（按 version 递增）
/// - 必须保证事件的幂等性（相同 event_id 不会重复存储）
/// - 建议实现索引以支持快速查询（aggregate_id, version）
///
/// ## 使用示例
///
/// ```no_run
/// use async_trait::async_trait;
/// use flare_im_core_sdk::domain::repository::EventStore;
/// use flare_im_core_sdk::domain::event::DomainEvent;
/// use std::sync::Arc;
///
/// struct MyEventStore { /* ... */ }
///
/// #[async_trait]
/// impl EventStore for MyEventStore {
///     async fn append(&self, event: DomainEvent) -> anyhow::Result<()> {
///         // 实现存储逻辑
///         Ok(())
///     }
///
///     async fn load_stream(&self, aggregate_id: &str) -> anyhow::Result<Vec<DomainEvent>> {
///         // 实现查询逻辑
///         Ok(vec![])
///     }
///
///     async fn load_stream_from_version(
///         &self,
///         aggregate_id: &str,
///         from_version: u64,
///     ) -> anyhow::Result<Vec<DomainEvent>> {
///         // 实现查询逻辑
///         Ok(vec![])
///     }
/// }
/// ```
#[async_trait]
pub trait EventStore: Send + Sync {
    /// 追加事件到事件流
    ///
    /// # 参数
    /// * `event` - 要存储的领域事件
    ///
    /// # 返回
    /// * `Ok(())` - 存储成功
    /// * `Err` - 存储失败
    async fn append(&self, event: DomainEvent) -> anyhow::Result<()>;
    
    /// 加载聚合根的所有事件
    ///
    /// # 参数
    /// * `aggregate_id` - 聚合根 ID
    ///
    /// # 返回
    /// * `Ok(Vec<DomainEvent>)` - 事件列表（按 version 排序）
    /// * `Err` - 查询失败
    async fn load_stream(&self, aggregate_id: &str) -> anyhow::Result<Vec<DomainEvent>>;
    
    /// 从指定版本开始加载事件
    ///
    /// # 参数
    /// * `aggregate_id` - 聚合根 ID
    /// * `from_version` - 起始版本号（不包含）
    ///
    /// # 返回
    /// * `Ok(Vec<DomainEvent>)` - 事件列表（按 version 排序）
    /// * `Err` - 查询失败
    async fn load_stream_from_version(
        &self,
        aggregate_id: &str,
        from_version: u64,
    ) -> anyhow::Result<Vec<DomainEvent>>;
}
