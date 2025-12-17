//! 连接相关命令

use anyhow::Result;

/// 登录命令
#[derive(Debug, Clone)]
pub struct LoginCommand {
    pub user_id: String,
    pub token: String,
}

/// 登出命令
#[derive(Debug, Clone)]
pub struct LogoutCommand;

/// 设置加密命令
#[derive(Clone)]
pub struct SetCryptoCommand {
    pub crypto: std::sync::Arc<dyn crate::application::CryptoService>,
}

impl std::fmt::Debug for SetCryptoCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetCryptoCommand")
            .field("crypto", &"<CryptoService>")
            .finish()
    }
}
