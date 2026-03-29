use crate::domain::models::UserProfile;
use crate::error::Result;
use async_trait::async_trait;

/// 用户资料查询（只读）
#[async_trait]
pub trait UserReader: Send + Sync {
    async fn get(&self, user_id: &str) -> Result<Option<UserProfile>>;
}

/// 用户资料写操作
#[async_trait]
pub trait UserWriter: Send + Sync {
    async fn save_batch(&self, profiles: &[UserProfile]) -> Result<()>;
}
