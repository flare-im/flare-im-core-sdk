//! Extension 机制
//!
//! 业务能力通过 Extension 接入，必须复用 Core Sync 能力
//!
//! ## 核心设计
//!
//! 1. **SdkExtension Trait**: 扩展必须实现的接口
//! 2. **SdkContext**: 提供给扩展的核心能力（CommandBus, QueryBus, EventBus, Store, Session, Connection）
//! 3. **ExtensionRegistry**: 管理所有已注册的扩展

mod registry;
mod context;

pub use registry::ExtensionRegistry;
pub use context::SdkContext;

/// Extension 规范
///
/// 所有业务扩展必须实现此 trait
pub trait SdkExtension: Send + Sync {
    /// Extension 名称（唯一标识）
    fn name(&self) -> &'static str;
    
    /// 注册 Extension
    ///
    /// 扩展可以在这里注册自己的命令处理器、查询处理器、事件监听器等
    /// 通过 SdkContext 访问核心 SDK 能力
    ///
    /// # 参数
    /// * `ctx` - SDK 上下文，提供核心能力
    ///
    /// # 返回
    /// * `Ok(())` - 注册成功
    /// * `Err` - 注册失败
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 默认实现：什么都不做
        // 扩展可以重写此方法来实现自己的注册逻辑
        Ok(())
    }
    
    /// 返回 Extension 的同步规格
    ///
    /// 扩展可以声明需要同步的资源类型和同步模式
    /// - Bootstrap: 必须在 SDK Ready 前完成，失败则 SDK 不可用
    /// - Async: 异步同步，可以失败和重试
    fn sync_specs(&self) -> Vec<SyncSpec> {
        vec![]
    }
    
    /// Extension 初始化完成回调
    ///
    /// 在 Extension 注册完成后调用，可以在这里执行初始化逻辑
    fn on_initialized(&self, _ctx: &SdkContext) -> anyhow::Result<()> {
        Ok(())
    }
    
    /// Extension 销毁回调
    ///
    /// 在 Extension 卸载前调用，可以在这里执行清理逻辑
    fn on_destroyed(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// 同步规格
///
/// 定义扩展需要同步的资源类型和同步模式
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSpec {
    /// 同步类型（如 "friend_list", "group_list", "user_profile"）
    pub sync_type: String,
    
    /// 同步模式
    pub mode: ExtensionSyncMode,
    
    /// 同步优先级（数字越大优先级越高，默认 0）
    pub priority: i32,
}

impl SyncSpec {
    /// 创建新的同步规格
    pub fn new(sync_type: String, mode: ExtensionSyncMode) -> Self {
        Self {
            sync_type,
            mode,
            priority: 0,
        }
    }
    
    /// 创建带优先级的同步规格
    pub fn with_priority(sync_type: String, mode: ExtensionSyncMode, priority: i32) -> Self {
        Self {
            sync_type,
            mode,
            priority,
        }
    }
}

/// Extension 同步模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionSyncMode {
    /// Bootstrap Sync
    ///
    /// 必须在 SDK Ready 前完成，失败则 SDK 不可用
    /// 用于必须的数据同步（如好友列表、群组列表）
    Bootstrap,
    
    /// Async Sync
    ///
    /// 异步同步，可以失败和重试
    /// 用于非关键数据同步（如用户状态、群组信息）
    Async,
}

