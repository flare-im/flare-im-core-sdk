//! 会话命令定义（Session Command）
//!
//! 定义所有会话（登录/登出）相关的写操作命令

/// 登录命令
#[derive(Debug, Clone)]
pub struct LoginCommand {
    pub user_id: String,
    pub token: String,
}

/// 登出命令
#[derive(Debug, Clone)]
pub struct LogoutCommand;

/// 连接命令
#[derive(Debug, Clone)]
pub struct ConnectCommand;

/// 断开连接命令
#[derive(Debug, Clone)]
pub struct DisconnectCommand;
