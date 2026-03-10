use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::Result;

/// Token 提供者 trait — 支持自动刷新
pub trait TokenProvider: Send + Sync {
    fn get_token(&self) -> Result<String>;
    fn refresh_token(&self) -> impl std::future::Future<Output = Result<String>> + Send;
}

/// 认证中间件 — 管理 token 生命周期
pub struct AuthMiddleware {
    token: Arc<RwLock<String>>,
    user_id: Arc<RwLock<String>>,
}

impl AuthMiddleware {
    pub fn new() -> Self {
        Self {
            token: Arc::new(RwLock::new(String::new())),
            user_id: Arc::new(RwLock::new(String::new())),
        }
    }

    pub async fn set_credentials(&self, user_id: &str, token: &str) {
        *self.user_id.write().await = user_id.to_string();
        *self.token.write().await = token.to_string();
    }

    pub async fn token(&self) -> String {
        self.token.read().await.clone()
    }

    pub async fn user_id(&self) -> String {
        self.user_id.read().await.clone()
    }

    pub async fn update_token(&self, token: &str) {
        *self.token.write().await = token.to_string();
    }
}

impl Default for AuthMiddleware {
    fn default() -> Self { Self::new() }
}
