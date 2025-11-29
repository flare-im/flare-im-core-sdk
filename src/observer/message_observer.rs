//! 消息观察者接口
//! 
//! 统一的消息和事件处理接口，替代原来的 MessageHandler 和 MessageObserver

use crate::model::Message;
use crate::event::Event;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 消息观察者接口（统一的消息和事件处理接口）
/// 
/// 实现此接口以处理特定类型的消息或事件
/// 
/// 注意：此接口替代了原来的 MessageHandler，提供更统一的消息处理方式
#[async_trait]
pub trait MessageObserver: Send + Sync {
    /// 处理收到的消息
    /// 
    /// # 参数
    /// - `message`: 收到的消息
    /// 
    /// # 返回
    /// - `Result<bool>`: 返回 true 表示消息已处理，false 表示继续传递给其他观察者
    async fn on_message(&self, message: &Message) -> Result<bool> {
        let _ = message;
        Ok(false)
    }
    
    /// 处理事件
    /// 
    /// # 参数
    /// - `event`: 事件
    /// 
    /// # 返回
    /// - `Result<bool>`: 返回 true 表示事件已处理，false 表示继续传递给其他观察者
    async fn on_event(&self, event: &Event) -> Result<bool> {
        let _ = event;
        Ok(false)
    }
    
    /// 支持的消息类型（用于消息过滤）
    /// 
    /// 返回支持的消息类型列表，空列表表示支持所有类型
    /// 消息类型可以是：
    /// - "Text", "Image", "Video" 等标准类型
    /// - 自定义类型的字符串标识
    fn supported_message_types(&self) -> Vec<String> {
        vec![] // 默认支持所有类型
    }
    
    /// 观察者优先级
    /// 
    /// 数字越小优先级越高，默认返回 100
    fn priority(&self) -> u32 {
        100
    }
    
    /// 观察者名称（用于调试和日志）
    fn name(&self) -> &str {
        "UnknownObserver"
    }
}

/// 线程安全的消息观察者引用
pub type ArcMessageObserver = Arc<dyn MessageObserver>;

/// 消息观察者注册表
/// 
/// 管理所有注册的观察者，按优先级排序
pub struct MessageObserverRegistry {
    observers: Arc<RwLock<Vec<ArcMessageObserver>>>,
}

impl MessageObserverRegistry {
    /// 创建新的观察者注册表
    pub fn new() -> Self {
        Self {
            observers: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    /// 注册观察者
    /// 
    /// # 参数
    /// - `observer`: 要注册的观察者
    pub async fn register(&self, observer: ArcMessageObserver) {
        let mut observers = self.observers.write().await;
        observers.push(observer);
        // 按优先级排序（优先级小的在前）
        observers.sort_by_key(|o| o.priority());
    }
    
    /// 取消注册观察者
    /// 
    /// # 参数
    /// - `observer`: 要取消注册的观察者
    pub async fn unregister(&self, observer: &ArcMessageObserver) {
        let mut observers = self.observers.write().await;
        observers.retain(|o| !Arc::ptr_eq(o, observer));
    }
    
    /// 通知所有观察者处理消息
    /// 
    /// # 参数
    /// - `message`: 要处理的消息
    /// 
    /// # 返回
    /// - `Result<bool>`: 返回 true 表示至少有一个观察者处理了消息
    /// 
    /// 优化：缩小锁持有时间，快速释放锁
    pub async fn notify_message(&self, message: &Message) -> Result<bool> {
        // 快速获取观察者列表并释放锁
        let observers: Vec<_> = {
            let obs = self.observers.read().await;
            obs.clone() // 克隆观察者列表，避免持有锁
        };
        
        let mut handled = false;
        
        // 在锁外处理所有观察者
        for observer in observers.iter() {
            match observer.on_message(message).await {
                Ok(true) => {
                    handled = true;
                    // 如果观察者返回 true，表示消息已处理，可以停止传递
                    // 但为了灵活性，我们继续传递给所有观察者
                }
                Ok(false) => {
                    // 继续传递给下一个观察者
                }
                Err(e) => {
                    tracing::warn!(
                        observer = observer.name(),
                        error = %e,
                        "Observer failed to handle message"
                    );
                }
            }
        }
        
        Ok(handled)
    }
    
    /// 通知所有观察者处理事件
    /// 
    /// # 参数
    /// - `event`: 要处理的事件
    /// 
    /// # 返回
    /// - `Result<bool>`: 返回 true 表示至少有一个观察者处理了事件
    /// 
    /// 优化：缩小锁持有时间，快速释放锁
    pub async fn notify_event(&self, event: &Event) -> Result<bool> {
        // 快速获取观察者列表并释放锁
        let observers: Vec<_> = {
            let obs = self.observers.read().await;
            obs.clone() // 克隆观察者列表，避免持有锁
        };
        
        let mut handled = false;
        
        // 在锁外处理所有观察者
        for observer in observers.iter() {
            match observer.on_event(event).await {
                Ok(true) => {
                    handled = true;
                }
                Ok(false) => {
                    // 继续传递给下一个观察者
                }
                Err(e) => {
                    tracing::warn!(
                        observer = observer.name(),
                        error = %e,
                        "Observer failed to handle event"
                    );
                }
            }
        }
        
        Ok(handled)
    }
    
    /// 获取观察者数量
    pub async fn count(&self) -> usize {
        self.observers.read().await.len()
    }
}

impl Default for MessageObserverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

