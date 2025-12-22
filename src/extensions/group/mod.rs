//! 群组 Extension
//!
//! 示例 Extension 实现，展示如何通过 Extension 机制扩展业务能力

use crate::application::extension::{SdkExtension, SdkContext, SyncSpec, ExtensionSyncMode};

/// 群组 Extension
///
/// 示例 Extension 实现，展示如何通过 Extension 机制扩展业务能力
pub struct GroupExtension;

impl GroupExtension {
    pub fn new() -> Self {
        Self
    }
}

impl SdkExtension for GroupExtension {
    fn name(&self) -> &'static str {
        "group"
    }
    
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 注册群组相关的命令处理器、查询处理器等
        // 可以通过 ctx 访问核心能力：
        // - ctx.command_handler: 注册命令处理器
        // - ctx.query_handler: 注册查询处理器
        // - ctx.event_bus: 订阅/发布事件
        // - ctx.event_store: 持久化领域事件
        // - ctx.read_store: 查询数据
        // - ctx.sync_coordinator: 执行同步
        
        tracing::info!("Group extension registered");
        
        // TODO: 实现群组相关的业务逻辑注册
        // 例如：
        // - 注册群组命令处理器
        // - 注册群组查询处理器
        // - 订阅群组相关事件
        
        Ok(())
    }
    
    fn sync_specs(&self) -> Vec<SyncSpec> {
        vec![
            SyncSpec::with_priority(
                "group_list".to_string(),
                ExtensionSyncMode::Bootstrap, // 群组列表需要在 Bootstrap 时同步
                10, // 高优先级
            ),
            SyncSpec::new(
                "group_members".to_string(),
                ExtensionSyncMode::Async, // 群组成员异步同步
            ),
            SyncSpec::new(
                "group_info".to_string(),
                ExtensionSyncMode::Async, // 群组信息异步同步
            ),
        ]
    }
    
    fn on_initialized(&self, _ctx: &SdkContext) -> anyhow::Result<()> {
        tracing::info!("Group extension initialized");
        Ok(())
    }
}

impl Default for GroupExtension {
    fn default() -> Self {
        Self::new()
    }
}
