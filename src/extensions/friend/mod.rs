//! 好友 Extension
//!
//! 示例 Extension 实现，展示如何通过 Extension 机制扩展业务能力

use crate::application::extension::{SdkExtension, SdkContext, SyncSpec, ExtensionSyncMode};

/// 好友 Extension
///
/// 示例 Extension 实现，展示如何通过 Extension 机制扩展业务能力
pub struct FriendExtension;

impl FriendExtension {
    pub fn new() -> Self {
        Self
    }
}

impl SdkExtension for FriendExtension {
    fn name(&self) -> &'static str {
        "friend"
    }
    
    fn register(&self, ctx: &mut SdkContext) -> anyhow::Result<()> {
        // 注册好友相关的命令处理器、查询处理器等
        // 可以通过 ctx 访问核心能力
        
        tracing::info!("Friend extension registered");
        
        // TODO: 实现好友相关的业务逻辑注册
        // 例如：
        // - 注册好友命令处理器
        // - 注册好友查询处理器
        // - 订阅好友相关事件
        
        Ok(())
    }
    
    fn sync_specs(&self) -> Vec<SyncSpec> {
        vec![
            SyncSpec::with_priority(
                "friend_list".to_string(),
                ExtensionSyncMode::Bootstrap, // 好友列表需要在 Bootstrap 时同步
                10, // 高优先级
            ),
            SyncSpec::new(
                "friend_status".to_string(),
                ExtensionSyncMode::Async, // 好友状态异步同步
            ),
        ]
    }
    
    fn on_initialized(&self, _ctx: &SdkContext) -> anyhow::Result<()> {
        tracing::info!("Friend extension initialized");
        Ok(())
    }
}

impl Default for FriendExtension {
    fn default() -> Self {
        Self::new()
    }
}
