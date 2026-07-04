use crate::domain::UserProfile;
use crate::shared::error::Result;
use async_trait::async_trait;

/// 用户资料查询（只读）
#[async_trait]
pub trait UserReader: Send + Sync {
    async fn get(&self, user_id: &str) -> Result<Option<UserProfile>>;

    /// 批量查询用户资料（按 user_id 返回 map）。默认逐个 `get`（兼容既有实现），
    /// 持久化实现应**覆盖为单次批量查询**，避免时间线打开时按消息逐条查发送者资料（N+1）。
    async fn get_many(
        &self,
        user_ids: &[String],
    ) -> Result<std::collections::HashMap<String, UserProfile>> {
        let mut out = std::collections::HashMap::with_capacity(user_ids.len());
        for id in user_ids {
            if let Some(profile) = self.get(id).await? {
                out.insert(id.clone(), profile);
            }
        }
        Ok(out)
    }
}

/// 用户资料写操作
#[async_trait]
pub trait UserWriter: Send + Sync {
    async fn save_batch(&self, profiles: &[UserProfile]) -> Result<()>;
}
