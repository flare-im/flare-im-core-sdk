//! Tauri `State`：持有唯一 [`IMClient`]。
//!
//! **性能**：命令热路径仅为 `IMClient::clone`（内部 `Arc`），无绑定层锁。登录后事件循环使用 [`tauri::AppHandle`]，由 `sdk_login` 参数由运行时注入，不写入本结构。

use flare_im_core_sdk::client::{IMClient, SdkConfigOverlay};

/// 由 `tauri::Builder::manage(SdkState::new())` 注入。
pub struct SdkState {
    client: IMClient,
}

impl SdkState {
    pub fn new() -> Self {
        Self {
            client: IMClient::new(),
        }
    }

    /// O(1)：`IMClient` 为 `Arc` 浅拷贝。
    #[inline]
    pub fn client(&self) -> IMClient {
        self.client.clone()
    }

    pub async fn set_config(
        &self,
        environment: Option<String>,
        sdk_config: Option<SdkConfigOverlay>,
    ) -> flare_im_core_sdk::Result<()> {
        self.client.init(environment, sdk_config).await
    }

    pub async fn config_snapshot(&self) -> (Option<String>, Option<SdkConfigOverlay>) {
        self.client.config_snapshot().await
    }

    pub async fn logout(&self) -> flare_im_core_sdk::Result<()> {
        self.client.logout().await
    }
}

impl Default for SdkState {
    fn default() -> Self {
        Self::new()
    }
}
