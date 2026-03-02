//! Extension Registry
//!
//! 管理所有已注册的扩展

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use crate::application::extension::{SdkExtension, SyncSpec};

/// Extension Registry
///
/// 管理所有已注册的扩展，支持动态注册和查询
pub struct ExtensionRegistry {
    /// 已注册的扩展（按名称索引）
    extensions: Arc<RwLock<HashMap<String, Arc<dyn SdkExtension>>>>,
    
    /// 所有扩展的同步规格（合并后的）
    all_sync_specs: Arc<RwLock<Vec<SyncSpec>>>,
}

impl ExtensionRegistry {
    /// 创建新的 Extension Registry
    pub fn new() -> Self {
        Self {
            extensions: Arc::new(RwLock::new(HashMap::new())),
            all_sync_specs: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// 注册扩展
    ///
    /// # 参数
    /// * `extension` - 要注册的扩展
    ///
    /// # 返回
    /// * `Ok(())` - 注册成功
    /// * `Err` - 如果扩展名称已存在
    pub async fn register_extension(
        &self,
        extension: Arc<dyn SdkExtension>,
    ) -> anyhow::Result<()> {
        let name = extension.name().to_string();
        
        let mut extensions = self.extensions.write().await;
        
        // 检查是否已注册
        if extensions.contains_key(&name) {
            return Err(anyhow::anyhow!("Extension '{}' is already registered", name));
        }
        
        // 注册扩展
        extensions.insert(name.clone(), extension.clone());
        
        // 更新同步规格
        let mut sync_specs = self.all_sync_specs.write().await;
        sync_specs.extend(extension.sync_specs());
        
        tracing::info!("Extension '{}' registered successfully", name);
        
        Ok(())
    }
    
    /// 获取扩展
    pub async fn get_extension(&self, name: &str) -> Option<Arc<dyn SdkExtension>> {
        let extensions = self.extensions.read().await;
        extensions.get(name).cloned()
    }
    
    /// 获取所有已注册的扩展名称
    pub async fn list_extensions(&self) -> Vec<String> {
        let extensions = self.extensions.read().await;
        extensions.keys().cloned().collect()
    }
    
    /// 获取所有扩展的同步规格
    pub async fn get_all_sync_specs(&self) -> Vec<SyncSpec> {
        let sync_specs = self.all_sync_specs.read().await;
        sync_specs.clone()
    }
    
    /// 获取 Bootstrap 模式的同步规格
    pub async fn get_bootstrap_sync_specs(&self) -> Vec<SyncSpec> {
        let sync_specs = self.all_sync_specs.read().await;
        sync_specs
            .iter()
            .filter(|spec| matches!(spec.mode, crate::application::extension::ExtensionSyncMode::Bootstrap))
            .cloned()
            .collect()
    }
    
    /// 获取 Async 模式的同步规格
    pub async fn get_async_sync_specs(&self) -> Vec<SyncSpec> {
        let sync_specs = self.all_sync_specs.read().await;
        sync_specs
            .iter()
            .filter(|spec| matches!(spec.mode, crate::application::extension::ExtensionSyncMode::Async))
            .cloned()
            .collect()
    }
    
    /// 执行扩展的 Bootstrap Sync
    pub async fn execute_extension_bootstrap_sync(&self) -> anyhow::Result<()> {
        let specs = self.get_bootstrap_sync_specs().await;
        
        // 按优先级排序
        let mut sorted_specs = specs;
        sorted_specs.sort_by(|a, b| b.priority.cmp(&a.priority));
        
        for spec in sorted_specs {
            tracing::info!("Executing extension bootstrap sync: {}", spec.sync_type);
            // 这里我们只是记录日志，实际上扩展的同步逻辑应该由扩展自己实现
            // 或者通过 SdkContext 提供的回调来执行
            // 由于 SdkExtension trait 目前没有定义具体的同步执行方法，
            // 我们假设扩展在 register 时已经设置好了相关的监听或处理器
            // 或者是未来版本中 SdkExtension 会增加 execute_sync(spec) 方法
            
            // 为了模拟，我们这里假设成功
            tracing::debug!("Extension bootstrap sync logic for {} should be executed here", spec.sync_type);
        }
        
        Ok(())
    }

    /// 检查扩展是否已注册
    pub async fn is_registered(&self, name: &str) -> bool {
        let extensions = self.extensions.read().await;
        extensions.contains_key(name)
    }
    
    /// 取消注册扩展
    pub async fn unregister_extension(&self, name: &str) -> anyhow::Result<()> {
        let mut extensions = self.extensions.write().await;
        
        if let Some(_extension) = extensions.remove(name) {
            // 重新计算同步规格（移除该扩展的规格）
            let mut sync_specs = self.all_sync_specs.write().await;
            sync_specs.clear();
            
            // 重新收集所有扩展的同步规格
            for ext in extensions.values() {
                sync_specs.extend(ext.sync_specs());
            }
            
            tracing::info!("Extension '{}' unregistered successfully", name);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Extension '{}' is not registered", name))
        }
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
