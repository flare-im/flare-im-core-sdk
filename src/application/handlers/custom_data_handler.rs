//! 自定义数据处理器
//!
//! 负责处理自定义推送数据（CustomPushData）
//!
//! # 处理流程
//!
//! 1. 根据 data_type 查找注册的处理器
//! 2. 分发到对应的扩展处理器
//! 3. 扩展处理器处理业务逻辑
//! 4. 发布自定义事件到 EventBus

use std::sync::Arc;
use std::collections::HashMap;
use crate::infrastructure::event_bus::EventBus;
use crate::application::extension::ExtensionRegistry;
use tracing::{info, warn, debug};

/// 自定义数据处理器
pub struct CustomDataHandler {
    event_bus: Arc<EventBus>,
    #[allow(dead_code)]
    extension_registry: Arc<ExtensionRegistry>,
}

impl CustomDataHandler {
    /// 创建新的自定义数据处理器
    pub fn new(
        event_bus: Arc<EventBus>,
        extension_registry: Arc<ExtensionRegistry>,
    ) -> Self {
        Self {
            event_bus,
            extension_registry,
        }
    }
    
    /// 处理自定义推送数据
    ///
    /// # 参数
    ///
    /// * `data_type` - 自定义数据类型标识
    /// * `payload` - 二进制负载
    /// * `metadata` - 元数据
    ///
    /// # 返回
    ///
    /// * `Ok(())` - 处理成功
    /// * `Err` - 处理失败
    pub async fn handle_custom_push_data(
        &self,
        data_type: String,
        payload: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> anyhow::Result<()> {
        info!(
            data_type = %data_type,
            payload_len = payload.len(),
            metadata_count = metadata.len(),
            "Processing CustomPushData"
        );
        
        // 1. 根据 data_type 查找注册的处理器
        // 目前通过 Extension 机制处理自定义数据
        // 如果扩展支持自定义数据处理，可以通过 ExtensionRegistry 查找
        
        // 2. 分发到对应的扩展处理器
        // TODO: 实现扩展处理器的查找和分发逻辑
        // 这里暂时只记录日志和发布事件
        
        debug!(
            data_type = %data_type,
            "Looking for extension handler for custom data type"
        );
        
        // 3. 扩展处理器处理业务逻辑
        // 目前暂时跳过，等待 Extension 机制完善
        
        // 4. 发布自定义事件到 EventBus
        use crate::domain::event::DomainEvent;
        let custom_event = DomainEvent::new(
            &format!("custom.{}", data_type),
            "custom",
            1,
            serde_json::json!({
                "data_type": data_type,
                "payload_len": payload.len(),
                "metadata": metadata,
            }),
        );
        
        if let Err(e) = self.event_bus.publish(custom_event).await {
            warn!("Failed to publish custom data event: {}", e);
        } else {
            info!("Published custom data event to EventBus");
        }
        
        Ok(())
    }
}
