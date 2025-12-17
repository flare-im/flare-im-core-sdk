//! 观察者模式模块
//!
//! 提供消息观察者接口，允许用户注册自定义的消息处理器

pub mod message_observer;

pub use message_observer::{ArcMessageObserver, MessageObserver, MessageObserverRegistry};
