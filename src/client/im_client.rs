//! [`IMClient`]：初始化、登录、登出与消息/会话 API（单一入口）。
//!
//! 内部为 [`tokio::sync::RwLock`]：同步读路径使用 `try_read`（禁止 `blocking_read`），避免在 `#[tokio::main]` 或异步任务中 panic。

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::client::builder::IMClientBuilder;
use crate::client::lifecycle::{
    default_ws_url, merge_sdk_config, parse_data_url_to_path, resolve_connect_token, LoginDbKind,
    SdkConfigOverlay,
};
use crate::core::{SdkEngine, SdkState};
use crate::error::ErrorCode;
use crate::event::EventBus;
use crate::client::api::{ConversationApi, MessageApi, MessageBuildApi};
use crate::store::StoreProvider;
use crate::util::generate_test_token as util_generate_test_token;
use crate::FlareError;
use crate::Result;

#[derive(Default)]
pub(crate) struct IMClientInner {
    pub environment: Option<String>,
    pub sdk_config: Option<SdkConfigOverlay>,
    pub data_root: Option<PathBuf>,
    pub current_user_id: Option<String>,
    pub engine: Option<SdkEngine>,
    pub message_api: Option<MessageApi>,
    pub message_build_api: Option<Arc<MessageBuildApi>>,
    pub conversation_api: Option<Arc<ConversationApi>>,
}

/// 唯一 SDK 句柄：[`Self::init`] → [`Self::login`]，或 [`Self::builder`] → [`Self::connect`]。
#[derive(Clone)]
pub struct IMClient {
    pub(crate) inner: Arc<RwLock<IMClientInner>>,
}

impl IMClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(IMClientInner::default())),
        }
    }

    pub fn builder() -> IMClientBuilder {
        IMClientBuilder::new()
    }

    pub(crate) fn from_inner(inner: IMClientInner) -> Self {
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    pub(crate) fn into_inner(self) -> IMClientInner {
        match Arc::try_unwrap(self.inner) {
            Ok(rw) => rw.into_inner(),
            Err(_) => panic!("IMClient must be uniquely owned"),
        }
    }

    fn not_connected() -> FlareError {
        FlareError::localized(ErrorCode::NotConnected, "SDK not connected")
    }

    fn lock_busy() -> FlareError {
        FlareError::localized(ErrorCode::InternalError, "IMClient lock busy")
    }

    /// 同步 API 使用的读锁：在 Tokio worker 上 **禁止** `blocking_read`，必须用 `try_read`。
    pub(crate) fn read_inner(
        &self,
    ) -> Result<tokio::sync::RwLockReadGuard<'_, IMClientInner>> {
        self.inner.try_read().map_err(|_| Self::lock_busy())
    }

    pub(crate) fn with_engine<R>(&self, f: impl FnOnce(&SdkEngine) -> R) -> Result<R> {
        let g = self.read_inner()?;
        let e = g.engine.as_ref().ok_or_else(Self::not_connected)?;
        Ok(f(e))
    }

    pub async fn init(
        &self,
        environment: Option<String>,
        sdk_config: Option<SdkConfigOverlay>,
    ) -> Result<()> {
        let mut g = self.inner.write().await;
        g.environment = environment;
        g.data_root = None;
        if let Some(ref cfg) = sdk_config {
            if let Some(ref url) = cfg.data_url {
                let path = parse_data_url_to_path(url)?;
                std::fs::create_dir_all(&path).map_err(|e| {
                    FlareError::localized(
                        ErrorCode::InvalidParameter,
                        format!("data_url create_dir_all failed: {}", e),
                    )
                })?;
                g.data_root = Some(path);
            }
        }
        g.sdk_config = sdk_config;
        Ok(())
    }

    pub async fn data_root(&self) -> Option<PathBuf> {
        self.inner.read().await.data_root.clone()
    }

    pub async fn resolve_data_subpath(
        &self,
        relative: impl AsRef<std::path::Path>,
    ) -> Result<PathBuf> {
        let root = self.data_root().await.ok_or_else(|| {
            FlareError::localized(
                ErrorCode::InvalidParameter,
                "init: set sdkConfig.dataUrl before resolving subpaths",
            )
        })?;
        let p = root.join(relative.as_ref());
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                FlareError::localized(
                    ErrorCode::InvalidParameter,
                    format!("resolve_data_subpath create_dir_all failed: {}", e),
                )
            })?;
        }
        Ok(p)
    }

    pub async fn config_snapshot(&self) -> (Option<String>, Option<SdkConfigOverlay>) {
        let g = self.inner.read().await;
        (g.environment.clone(), g.sdk_config.clone())
    }

    pub async fn is_connected(&self) -> bool {
        self.inner.read().await.current_user_id.is_some()
    }

    pub async fn current_user_id(&self) -> Option<String> {
        self.inner.read().await.current_user_id.clone()
    }

    pub fn generate_test_token(
        secret: &str,
        issuer: &str,
        user_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<String> {
        let s = if secret.is_empty() {
            "insecure-secret"
        } else {
            secret
        };
        let i = if issuer.is_empty() {
            "flare-im-core"
        } else {
            issuer
        };
        util_generate_test_token(s, i, user_id, 3600, None, tenant_id)
    }

    pub async fn logout(&self) -> Result<()> {
        let engine = {
            let mut g = self.inner.write().await;
            g.current_user_id = None;
            g.engine.take()
        };
        if let Some(mut e) = engine {
            e.disconnect().await?;
        }
        let mut g = self.inner.write().await;
        g.message_api = None;
        g.message_build_api = None;
        g.conversation_api = None;
        Ok(())
    }

    pub async fn login<F>(
        &self,
        user_id: &str,
        explicit_token: Option<&str>,
        db: LoginDbKind,
        before_connect: F,
    ) -> Result<()>
    where
        F: FnOnce(crate::event::EventBus, Arc<dyn crate::store::MessageStore>) + Send + 'static,
    {
        self.logout().await?;
        let snap = {
            let g = self.inner.read().await;
            (
                g.environment.clone(),
                g.sdk_config.clone(),
                g.data_root.clone(),
            )
        };
        let stores = match db {
            #[cfg(feature = "lifecycle-sqlite")]
            LoginDbKind::Sqlite => {
                let base = snap.2.clone().ok_or_else(|| {
                    FlareError::localized(
                        ErrorCode::InvalidParameter,
                        "init: set sdkConfig.dataUrl before SQLite login",
                    )
                })?;
                crate::util::sqlite_store::open_sqlite_store_for_user(&base, user_id).await?
            }
            LoginDbKind::IndexedDb(stores) => stores,
        };
        let ws_url = default_ws_url(snap.1.as_ref());
        let config = merge_sdk_config(&ws_url, snap.1.as_ref());
        let child = IMClientBuilder::new().config(config).stores(stores).build();
        let bus = child.bus()?.clone();
        let msg_store = child.stores()?.messages.clone();
        before_connect(bus, msg_store);
        child.connect(user_id, explicit_token).await?;
        let mut inner = child.into_inner();
        inner.environment = snap.0;
        inner.sdk_config = snap.1;
        inner.data_root = snap.2;
        inner.current_user_id = Some(user_id.to_string());
        *self.inner.write().await = inner;
        Ok(())
    }

    pub async fn connect(&self, user_id: &str, explicit_token: Option<&str>) -> Result<()> {
        let token = resolve_connect_token(user_id, explicit_token)?;
        let engine = {
            let mut g = self.inner.write().await;
            g.engine.take()
        };
        let mut e = engine.ok_or_else(|| {
            FlareError::localized(ErrorCode::NotConnected, "no engine; use builder or login")
        })?;
        e.connect(user_id, &token).await?;
        let mut g = self.inner.write().await;
        g.engine = Some(e);
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        let engine = {
            let mut g = self.inner.write().await;
            g.engine.take()
        };
        if let Some(mut e) = engine {
            e.disconnect().await?;
        }
        Ok(())
    }

    pub fn state(&self) -> SdkState {
        match self.inner.try_read() {
            Ok(g) => g
                .engine
                .as_ref()
                .map(|e| e.state())
                .unwrap_or(SdkState::Disconnected),
            Err(_) => SdkState::Disconnected,
        }
    }

    pub fn message(&self) -> Result<MessageApi> {
        self.read_inner()?
            .message_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    pub fn message_build(&self) -> Result<Arc<MessageBuildApi>> {
        self.read_inner()?
            .message_build_api
            .clone()
            .ok_or_else(Self::not_connected)
    }

    pub fn conversation(&self) -> Result<ConversationApi> {
        self.read_inner()?
            .conversation_api
            .as_ref()
            .map(|a| a.as_ref().clone())
            .ok_or_else(Self::not_connected)
    }

    pub fn bus(&self) -> Result<EventBus> {
        self.with_engine(|e| e.bus().clone())
    }

    pub fn stores(&self) -> Result<StoreProvider> {
        self.with_engine(|e| e.stores().clone())
    }

    pub async fn sync_conversation(&self, conversation_id: &str) -> Result<()> {
        let runner = self.with_engine(|e| e.session_sync_runner())?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner.request_message_sync(conversation_id).await
    }

    pub async fn sync_messages(
        &self,
        conversation_id: &str,
        last_seq: u64,
        limit: i32,
    ) -> Result<()> {
        let runner = self.with_engine(|e| e.session_sync_runner())?
            .ok_or_else(|| FlareError::localized(ErrorCode::NotConnected, "未配置同步"))?;
        runner
            .request_message_sync_from_seq(conversation_id, last_seq, limit)
            .await
    }

    pub async fn mark_session_read(&self, conversation_id: &str, read_seq: u64) -> Result<()> {
        let m = self.message()?;
        let c = self.conversation()?;
        m.mark_read(conversation_id, read_seq).await?;
        c.mark_read(conversation_id, read_seq).await?;
        if let Some(runner) = self.with_engine(|e| e.session_sync_runner())? {
            runner.send_read_ack(conversation_id, read_seq).await?;
        }
        Ok(())
    }

    pub async fn set_conversation_input_state(
        &self,
        conversation_id: &str,
        is_typing: bool,
    ) -> Result<()> {
        self.message()?.typing(conversation_id, is_typing).await
    }
}

impl Default for IMClient {
    fn default() -> Self {
        Self::new()
    }
}
