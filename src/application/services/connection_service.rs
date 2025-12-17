//! 连接应用服务
//!
//! 编排连接相关的业务逻辑

use crate::api::LoginResult;
use crate::application::handlers::ConnectionCommandHandler;
use anyhow::Result;
use std::sync::Arc;

/// 连接应用服务
///
/// 编排连接相关的业务逻辑
pub struct ConnectionService {
    command_handler: Arc<ConnectionCommandHandler>,
}

impl ConnectionService {
    pub fn new(command_handler: Arc<ConnectionCommandHandler>) -> Self {
        Self { command_handler }
    }

    /// 登录
    pub async fn login(&self, user_id: &str, token: &str) -> Result<LoginResult> {
        use crate::application::commands::connection::LoginCommand;
        self.command_handler
            .handle_login(LoginCommand {
                user_id: user_id.to_string(),
                token: token.to_string(),
            })
            .await
    }

    /// 登出
    pub async fn logout(&self) -> Result<()> {
        use crate::application::commands::connection::LogoutCommand;
        self.command_handler.handle_logout(LogoutCommand).await
    }

    /// 设置加密
    pub async fn set_crypto(
        &self,
        crypto: Arc<dyn crate::application::CryptoService>,
    ) -> Result<()> {
        use crate::application::commands::connection::SetCryptoCommand;
        self.command_handler
            .handle_set_crypto(SetCryptoCommand { crypto })
            .await
    }
}
