//! 快照存储接口
//!
//! 用于存储聚合根的快照，减少事件回放的开销。
//! 快照是可选的，如果未实现，SDK 会通过事件回放重建聚合根。

use async_trait::async_trait;

/// 快照存储接口
///
/// 用于存储聚合根的快照，减少事件回放的开销。
/// 快照是可选的，如果未实现，SDK 会通过事件回放重建聚合根。
///
/// ## 实现要求
///
/// - 快照应该包含版本号，用于验证一致性
/// - 建议定期清理旧快照
///
/// ## 使用示例
///
/// ```no_run
/// use async_trait::async_trait;
/// use flare_im_core_sdk::domain::repository::SnapshotStore;
/// use std::sync::Arc;
///
/// struct MySnapshotStore { /* ... */ }
///
/// #[async_trait]
/// impl SnapshotStore<MyAggregate> for MySnapshotStore {
///     async fn load(&self, aggregate_id: &str) -> anyhow::Result<Option<MyAggregate>> {
///         // 实现加载逻辑
///         Ok(None)
///     }
///
///     async fn save(
///         &self,
///         aggregate_id: &str,
///         aggregate: &MyAggregate,
///         version: u64,
///     ) -> anyhow::Result<()> {
///         // 实现保存逻辑
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait SnapshotStore<A>: Send + Sync
where
    A: Send + Sync + Clone,
{
    /// 加载快照
    ///
    /// # 参数
    /// * `aggregate_id` - 聚合根 ID
    ///
    /// # 返回
    /// * `Ok(Some(A))` - 找到快照
    /// * `Ok(None)` - 未找到快照
    /// * `Err` - 查询失败
    async fn load(&self, aggregate_id: &str) -> anyhow::Result<Option<A>>;
    
    /// 保存快照
    ///
    /// # 参数
    /// * `aggregate_id` - 聚合根 ID
    /// * `aggregate` - 聚合根实例
    /// * `version` - 版本号
    ///
    /// # 返回
    /// * `Ok(())` - 保存成功
    /// * `Err` - 保存失败
    async fn save(&self, aggregate_id: &str, aggregate: &A, version: u64) -> anyhow::Result<()>;
}
