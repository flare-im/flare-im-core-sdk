use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::domain::{UserProfile, UserReader, UserWriter};
use crate::error::{ErrorCode, FlareError, Result};

pub struct SqliteUserProfileRepo {
    pool: SqlitePool,
}

impl SqliteUserProfileRepo {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserReader for SqliteUserProfileRepo {
    async fn get(&self, user_id: &str) -> Result<Option<UserProfile>> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT nickname, avatar_url FROM user_profiles WHERE user_id = ?")
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(row.map(|(nickname, avatar_url)| UserProfile {
            user_id: user_id.to_string(),
            nickname,
            avatar_url,
        }))
    }
}

#[async_trait]
impl UserWriter for SqliteUserProfileRepo {
    async fn save_batch(&self, profiles: &[UserProfile]) -> Result<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        for p in profiles {
            sqlx::query(
                r#"INSERT OR REPLACE INTO user_profiles (user_id, nickname, avatar_url)
                   VALUES (?, ?, ?)"#,
            )
            .bind(&p.user_id)
            .bind(&p.nickname)
            .bind(&p.avatar_url)
            .execute(&mut *tx)
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| FlareError::localized(ErrorCode::DatabaseError, e.to_string()))?;
        Ok(())
    }
}
