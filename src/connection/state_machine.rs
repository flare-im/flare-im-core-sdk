//! 连接状态机
//!
//! 管理连接状态的转换，确保状态转换的原子性和有效性

use crate::connection::manager::ConnectionState;
use crate::event::{EventBus, Event, ConnectionEvent};
use anyhow::{Result, Context};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn, info};

/// 状态转换事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateTransition {
    /// 开始连接
    Connect,
    /// 连接已建立
    Connected,
    /// 开始认证
    Authenticate,
    /// 认证成功
    Authenticated,
    /// 断开连接
    Disconnect,
    /// 开始重连
    Reconnect,
    /// 连接错误
    Error,
}

/// 状态机配置
pub struct StateMachineConfig {
    /// 是否在状态转换时发布事件
    pub publish_events: bool,
    /// 是否记录状态转换日志
    pub log_transitions: bool,
}

impl Default for StateMachineConfig {
    fn default() -> Self {
        Self {
            publish_events: true,
            log_transitions: true,
        }
    }
}

/// 连接状态机
/// 
/// 管理连接状态的转换，确保：
/// 1. 状态转换的原子性
/// 2. 只允许有效的状态转换
/// 3. 状态转换时可以触发事件
pub struct ConnectionStateMachine {
    /// 当前状态
    state: Arc<RwLock<ConnectionState>>,
    
    /// 事件总线（用于发布状态变化事件）
    event_bus: Option<Arc<EventBus>>,
    
    /// 配置
    config: StateMachineConfig,
}

impl ConnectionStateMachine {
    /// 创建新的状态机
    pub fn new(
        initial_state: ConnectionState,
        event_bus: Option<Arc<EventBus>>,
        config: StateMachineConfig,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(initial_state)),
            event_bus,
            config,
        }
    }
    
    /// 获取当前状态（无锁读取）
    pub async fn current_state(&self) -> ConnectionState {
        *self.state.read().await
    }
    
    /// 尝试状态转换
    /// 
    /// # 参数
    /// - `transition`: 状态转换事件
    /// 
    /// # 返回
    /// - `Ok(ConnectionState)`: 转换成功，返回新状态
    /// - `Err`: 转换失败（无效的状态转换）
    pub async fn transition(&self, transition: StateTransition) -> Result<ConnectionState> {
        let current = *self.state.read().await;
        
        // 验证状态转换是否有效
        let new_state = Self::validate_transition(current, transition)
            .with_context(|| format!(
                "Invalid state transition: {:?} -> {:?}",
                current, transition
            ))?;
        
        // 执行状态转换（原子操作）
        {
            let mut state = self.state.write().await;
            *state = new_state;
        }
        
        // 记录日志
        if self.config.log_transitions {
            info!(
                from = ?current,
                to = ?new_state,
                transition = ?transition,
                "Connection state transition"
            );
        }
        
        // 发布事件
        if self.config.publish_events {
            self.publish_state_event(new_state, transition).await;
        }
        
        Ok(new_state)
    }
    
    /// 验证状态转换是否有效
    /// 
    /// 定义允许的状态转换规则：
    /// - Disconnected -> Connecting (Connect)
    /// - Connecting -> Connected (Connected)
    /// - Connected -> Authenticating (Authenticate)
    /// - Authenticating -> Authenticated (Authenticated)
    /// - Any -> Disconnected (Disconnect)
    /// - Authenticated -> Reconnecting (Reconnect)
    /// - Reconnecting -> Connected (Connected)
    fn validate_transition(
        current: ConnectionState,
        transition: StateTransition,
    ) -> Result<ConnectionState> {
        let new_state = match (current, transition) {
            // 正常连接流程
            (ConnectionState::Disconnected, StateTransition::Connect) => {
                ConnectionState::Connecting
            }
            (ConnectionState::Connecting, StateTransition::Connected) => {
                ConnectionState::Connected
            }
            (ConnectionState::Connected, StateTransition::Authenticate) => {
                ConnectionState::Authenticating
            }
            (ConnectionState::Authenticating, StateTransition::Authenticated) => {
                ConnectionState::Authenticated
            }
            
            // 重连流程
            (ConnectionState::Authenticated, StateTransition::Reconnect) => {
                ConnectionState::Reconnecting
            }
            (ConnectionState::Reconnecting, StateTransition::Connected) => {
                ConnectionState::Connected
            }
            (ConnectionState::Reconnecting, StateTransition::Authenticated) => {
                ConnectionState::Authenticated
            }
            
            // 断开连接（可以从任何状态断开）
            (_, StateTransition::Disconnect) => {
                ConnectionState::Disconnected
            }
            
            // 错误处理（可以从任何状态进入错误，然后断开）
            (_, StateTransition::Error) => {
                ConnectionState::Disconnected
            }
            
            // 快速路径：如果已经在目标状态，允许保持
            (ConnectionState::Connected, StateTransition::Connected) => {
                ConnectionState::Connected
            }
            (ConnectionState::Authenticated, StateTransition::Authenticated) => {
                ConnectionState::Authenticated
            }
            
            // 其他转换都是无效的
            _ => {
                return Err(anyhow::anyhow!(
                    "Invalid transition: {:?} -> {:?}",
                    current, transition
                ));
            }
        };
        
        Ok(new_state)
    }
    
    /// 发布状态变化事件
    async fn publish_state_event(&self, state: ConnectionState, transition: StateTransition) {
        if let Some(ref event_bus) = self.event_bus {
            let event = match (state, transition) {
                (ConnectionState::Connected, StateTransition::Connected) => {
                    Event::Connection(ConnectionEvent::Connected { protocol: None })
                }
                (ConnectionState::Authenticated, StateTransition::Authenticated) => {
                    Event::Connection(ConnectionEvent::Authenticated)
                }
                (ConnectionState::Disconnected, StateTransition::Disconnect) => {
                    Event::Connection(ConnectionEvent::Disconnected)
                }
                (ConnectionState::Reconnecting, StateTransition::Reconnect) => {
                    Event::Connection(ConnectionEvent::Reconnecting)
                }
                _ => {
                    // 其他状态变化不发布事件（或发布通用事件）
                    return;
                }
            };
            
            event_bus.publish(event);
        }
    }
    
    /// 强制设置状态（不验证，用于恢复或特殊情况）
    /// 
    /// ⚠️ 警告：此方法会跳过状态验证，只在特殊情况下使用
    pub async fn force_set_state(&self, new_state: ConnectionState) {
        let old_state = *self.state.read().await;
        
        {
            let mut state = self.state.write().await;
            *state = new_state;
        }
        
        warn!(
            from = ?old_state,
            to = ?new_state,
            "Force set connection state (bypassed validation)"
        );
    }
    
    /// 检查是否可以执行某个转换
    pub async fn can_transition(&self, transition: StateTransition) -> bool {
        let current = *self.state.read().await;
        Self::validate_transition(current, transition).is_ok()
    }
    
    /// 等待状态变为指定状态
    /// 
    /// # 参数
    /// - `target_state`: 目标状态
    /// - `timeout`: 超时时间
    /// 
    /// # 返回
    /// - `Ok(())`: 状态已变为目标状态
    /// - `Err`: 超时或状态转换失败
    pub async fn wait_for_state(
        &self,
        target_state: ConnectionState,
        timeout: std::time::Duration,
    ) -> Result<()> {
        use tokio::time::{sleep, Instant, timeout as tokio_timeout};
        
        let start = Instant::now();
        
        tokio_timeout(timeout, async {
            loop {
                let current = *self.state.read().await;
                if current == target_state {
                    return Ok(());
                }
                
                // 检查是否超时
                if start.elapsed() > timeout {
                    return Err(anyhow::anyhow!(
                        "Timeout waiting for state {:?}, current: {:?}",
                        target_state, current
                    ));
                }
                
                // 短暂等待后重试
                sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("Timeout waiting for state {:?}", target_state))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_state_machine_transitions() {
        let sm = ConnectionStateMachine::new(
            ConnectionState::Disconnected,
            None,
            StateMachineConfig::default(),
        );
        
        // 测试正常连接流程
        assert!(sm.transition(StateTransition::Connect).await.is_ok());
        assert_eq!(sm.current_state().await, ConnectionState::Connecting);
        
        assert!(sm.transition(StateTransition::Connected).await.is_ok());
        assert_eq!(sm.current_state().await, ConnectionState::Connected);
        
        assert!(sm.transition(StateTransition::Authenticate).await.is_ok());
        assert_eq!(sm.current_state().await, ConnectionState::Authenticating);
        
        assert!(sm.transition(StateTransition::Authenticated).await.is_ok());
        assert_eq!(sm.current_state().await, ConnectionState::Authenticated);
        
        // 测试无效转换
        assert!(sm.transition(StateTransition::Connect).await.is_err());
    }
    
    #[tokio::test]
    async fn test_state_machine_disconnect() {
        let sm = ConnectionStateMachine::new(
            ConnectionState::Authenticated,
            None,
            StateMachineConfig::default(),
        );
        
        // 可以从任何状态断开
        assert!(sm.transition(StateTransition::Disconnect).await.is_ok());
        assert_eq!(sm.current_state().await, ConnectionState::Disconnected);
    }
}

