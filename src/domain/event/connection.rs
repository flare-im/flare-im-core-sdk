//! Connection 领域事件
//!
//! 定义所有 Connection 聚合根相关的领域事件

use serde::{Deserialize, Serialize};

/// Connection 已连接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConnected {
    pub connection_id: String,
}

/// Connection 已断开
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDisconnected {
    pub reason: String,
}

/// Connection 重连中
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionReconnecting {
    pub attempt: u32,
}

/// Connection 重连成功
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionReconnected {
    pub connection_id: String,
    pub attempt: u32,
}

/// Connection 连接失败
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConnectFailed {
    pub error: String,
    pub attempt: u32,
}
