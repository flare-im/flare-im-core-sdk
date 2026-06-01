//! 用户资料变更 → 本地 IM 视图投影（user_profiles / 会话参与者 / 会话展示字段）。

use crate::domain::UserProfile;
use crate::error::Result;
use crate::infrastructure::persistence::StoreProvider;

/// 将用户资料写入本地 IM store，并同步关联的会话参与者与会话展示字段。
pub struct UserProfileProjectionApplier;

impl UserProfileProjectionApplier {
    pub async fn apply(stores: &StoreProvider, profile: &UserProfile) -> Result<Vec<String>> {
        stores
            .user_profiles_writer_or_memory()
            .save_batch(std::slice::from_ref(profile))
            .await?;
        Self::apply_local_views(stores, profile).await
    }

    /// 仅刷新参与者与会话展示字段（假定 `user_profiles` 已写入）。
    pub async fn apply_local_views(
        stores: &StoreProvider,
        profile: &UserProfile,
    ) -> Result<Vec<String>> {
        if let Some(participants) = &stores.conversation_participants {
            participants
                .patch_user_display(
                    &profile.user_id,
                    profile.display_name(),
                    &profile.avatar_url,
                )
                .await?;
        }

        stores
            .conversations
            .apply_user_profile_snapshot(
                &profile.user_id,
                profile.display_name(),
                &profile.avatar_url,
            )
            .await
    }
}
