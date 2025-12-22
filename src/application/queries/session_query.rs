//! 会话查询定义（Session Query）
//!
//! 定义所有会话（登录状态）相关的读操作查询

/// 查询会话状态
#[derive(Debug, Clone)]
pub struct GetSessionStateQuery;

/// 查询连接状态
#[derive(Debug, Clone)]
pub struct GetConnectionStateQuery;

/// 查询同步状态
#[derive(Debug, Clone)]
pub struct GetSyncStateQuery;
