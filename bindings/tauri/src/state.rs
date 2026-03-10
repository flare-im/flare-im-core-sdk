//! Tauri 管理的 SDK 状态：持有 IMClient 与配置，供 commands 使用。

use std::sync::Arc;
use tokio::sync::RwLock;

use flare_im_core_sdk::client::im_client::IMClient;

/// 内部状态（供 event forwarder 等只读使用）
pub(crate) struct SdkStateInner {
    pub(crate) client: Option<IMClient>,
    /// "development" | "production"，由前端传入；开发用 temp-data，生产用 Tauri 应用目录
    environment: Option<String>,
    /// 上层传入的 SdkConfig（含 ws_url 等），connect 时直接用于生成 SdkConfig
    sdk_config: Option<crate::model::SdkConfigOptions>,
    current_user_id: Option<String>,
    app_handle: Option<tauri::AppHandle>,
}

/// Tauri 侧 SDK 状态
///
/// 由 `tauri::Builder::manage(SdkState::new())` 注入，commands 通过 `State<SdkState>` 访问。
pub struct SdkState {
    pub(crate) inner: Arc<RwLock<SdkStateInner>>,
}

impl SdkState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SdkStateInner {
                client: None,
                environment: None,
                sdk_config: None,
                current_user_id: None,
                app_handle: None,
            })),
        }
    }

    /// 设置 Tauri AppHandle（用于向前端 emit 事件），由 sdk_init 调用
    pub async fn set_app_handle(&self, app: tauri::AppHandle) {
        let mut g = self.inner.write().await;
        g.app_handle = Some(app);
    }

    /// 设置环境与 SdkConfig（ws_url 等在 sdk_config 中），由 sdk_init 调用
    pub async fn set_config(
        &self,
        environment: Option<String>,
        sdk_config: Option<crate::model::SdkConfigOptions>,
    ) {
        let mut g = self.inner.write().await;
        g.environment = environment;
        g.sdk_config = sdk_config;
    }

    /// 设置当前用户（连接成功后）
    pub async fn set_current_user(&self, user_id: Option<String>) {
        let mut g = self.inner.write().await;
        g.current_user_id = user_id;
    }

    /// 获取 AppHandle 用于 emit
    pub async fn app_handle(&self) -> Option<tauri::AppHandle> {
        let g = self.inner.read().await;
        g.app_handle.clone()
    }

    /// 获取当前 user_id
    pub async fn current_user_id(&self) -> Option<String> {
        let g = self.inner.read().await;
        g.current_user_id.clone()
    }

    /// 在已连接时执行操作（支持 async 闭包；读锁会持有到 Future 完成）
    pub async fn with_client<T, F, E>(&self, f: F) -> std::result::Result<T, E>
    where
        F: for<'a> FnOnce(&'a IMClient) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<T, E>> + Send + 'a>>,
        E: From<String>,
    {
        let g = self.inner.read().await;
        let client = g.client.as_ref().ok_or_else(|| "SDK not connected".to_string())?;
        f(client).await
    }

    /// 获取 environment、sdk_config（用于 connect 时解析路径与 SdkConfig）
    pub async fn config(
        &self,
    ) -> (Option<String>, Option<crate::model::SdkConfigOptions>) {
        let g = self.inner.read().await;
        (g.environment.clone(), g.sdk_config.clone())
    }

    /// 设置已连接的 client（connect 成功后调用）
    pub async fn set_client(&self, client: IMClient) {
        let mut g = self.inner.write().await;
        g.client = Some(client);
    }

    /// 取出 client（logout/disconnect 时调用）
    pub async fn take_client(&self) -> Option<IMClient> {
        let mut g = self.inner.write().await;
        g.client.take()
    }
}

impl Default for SdkState {
    fn default() -> Self {
        Self::new()
    }
}
