//! 连接状态机 — 控制 SDK 与服务器的连接生命周期

use crate::shared::error::{FlareError, Result};

/// 连接状态（显式枚举，禁止隐式变更）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// 未连接
    Disconnected,
    /// 正在建立连接（含认证等待）
    Connecting,
    /// 已建连，尚未完成 bootstrap
    Connected,
    /// 就绪（bootstrap 完成，可收发）
    Ready,
    /// 断线重连中
    Reconnecting,
}

/// 触发连接状态转移的事件
#[derive(Clone, Debug)]
pub enum ConnectionEvent {
    /// 发起连接
    ConnectRequested,
    /// 连接成功（含 CONNACK）
    Connected,
    /// Bootstrap 完成
    BootstrapDone,
    /// 断开请求
    DisconnectRequested,
    /// 连接断开（含异常、踢下线）
    Disconnected,
    /// 检测到断线，发起重连
    ReconnectRequested,
}

/// 连接 FSM：仅做状态转移，不持有 IO
pub struct ConnectionFsm;

impl ConnectionFsm {
    /// 根据当前状态与事件计算下一状态；非法转移返回 Err
    pub fn transition(from: ConnectionState, event: &ConnectionEvent) -> Result<ConnectionState> {
        use ConnectionEvent as E;
        use ConnectionState as S;

        let next = match (from, event) {
            (S::Disconnected, E::ConnectRequested) => S::Connecting,
            (S::Connecting, E::Connected) => S::Connected,
            (S::Connecting, E::Disconnected) => S::Disconnected,
            (S::Connected, E::BootstrapDone) => S::Ready,
            (S::Connected, E::Disconnected) => S::Disconnected,
            (S::Ready, E::DisconnectRequested) => S::Disconnected,
            (S::Ready, E::Disconnected) => S::Disconnected,
            (S::Ready, E::ReconnectRequested) => S::Reconnecting,
            (S::Reconnecting, E::Connected) => S::Connected,
            (S::Reconnecting, E::Disconnected) => S::Disconnected,
            _ => {
                return Err(FlareError::system(format!(
                    "invalid connection transition: {:?} + {:?}",
                    from, event
                )));
            }
        };
        Ok(next)
    }
}
