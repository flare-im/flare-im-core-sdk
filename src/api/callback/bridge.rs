//! 回调桥接层实现
//!
//! 为所有 API trait 提供回调版本的桥接实现
//!
//! ## 设计说明
//!
//! 1. **零开销**: Rust 原生调用时直接使用 `Result<T>`，无额外开销
//! 2. **统一模式**: 所有 API 都提供 `*_with_callback` 版本
//! 3. **类型安全**: 通过泛型保证类型安全
//! 4. **异步友好**: 所有回调都在异步上下文中执行
//!
//! ## 使用方式
//!
//! ### Rust 原生（推荐）
//! ```rust,no_run
//! let result = client.login("user_123", "token").await?;
//! ```
//!
//! ### 回调版本（跨语言/异步场景）
//! ```rust,no_run
//! use flare_im_core_sdk::api::callback::*;
//!
//! let callback = callback!(|result| {
//!     match result {
//!         Ok(login_result) => println!("登录成功"),
//!         Err(e) => eprintln!("登录失败: {}", e),
//!     }
//! });
//!
//! client.login_with_callback("user_123", "token", callback).await;

use crate::api::callback::core::{Callback, CallbackBridge};
use crate::api::traits::{ConnectionApi, MessageApi, SessionApi};
use std::sync::Arc;

/// 连接管理 API 回调桥接
pub trait ConnectionApiCallback: ConnectionApi {
    /// 登录（回调版本）
    fn login_with_callback<C>(
        &self,
        user_id: &str,
        token: &str,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<crate::api::LoginResult> + 'static;

    /// 登出（回调版本）
    fn logout_with_callback<C>(
        &self,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static;

    /// 设置 AES-256 加密（回调版本）
    fn set_crypto_aes256_with_callback<C>(
        &self,
        key: &[u8],
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static;
}

impl<T: ConnectionApi> ConnectionApiCallback for T {
    fn login_with_callback<C>(
        &self,
        user_id: &str,
        token: &str,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<crate::api::LoginResult> + 'static,
    {
        let user_id = user_id.to_string();
        let token = token.to_string();
        // 直接调用，不使用 spawn（因为 self 的生命周期限制）
        // 注意：这要求 ConnectionApi::login 返回的 Future 是 Send 的
        Box::pin(async move {
            let result = ConnectionApi::login(self, &user_id, &token).await;
            match result {
                Ok(value) => callback.on_success(value),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }

    fn logout_with_callback<C>(
        &self,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static,
    {
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = ConnectionApi::logout(self).await;
            match result {
                Ok(()) => callback.on_success(()),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }

    fn set_crypto_aes256_with_callback<C>(
        &self,
        key: &[u8],
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static,
    {
        let key = key.to_vec();
        Box::pin(async move {
            // 直接调用并处理结果
            let result = ConnectionApi::set_crypto_aes256(self, &key).await;
            match result {
                Ok(()) => callback.on_success(()),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }
}

/// 会话管理 API 回调桥接
pub trait SessionApiCallback: SessionApi {
    /// 获取会话列表（回调版本）
    fn get_sessions_with_callback<C>(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<Vec<crate::application::vo::SessionVO>> + 'static;

    /// 创建会话（回调版本）
    fn create_session_with_callback<C>(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Option<Vec<String>>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<String> + 'static;

    /// 标记已读（回调版本）
    fn mark_read_with_callback<C>(
        &self,
        session_id: &str,
        message_seq: Option<i64>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static;
}

impl<T: SessionApi> SessionApiCallback for T {
    fn get_sessions_with_callback<C>(
        &self,
        filter: crate::infrastructure::storage::SessionFilter,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<Vec<crate::application::vo::SessionVO>> + 'static,
    {
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = SessionApi::get_sessions(self, filter).await;
            match result {
                Ok(value) => callback.on_success(value),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }

    fn create_session_with_callback<C>(
        &self,
        session_id: Option<String>,
        session_type: String,
        business_type: String,
        display_name: Option<String>,
        participants: Option<Vec<String>>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<String> + 'static,
    {
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = SessionApi::create_session(
                self,
                session_id,
                session_type,
                business_type,
                display_name,
                participants,
            )
            .await;
            match result {
                Ok(value) => callback.on_success(value),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }

    fn mark_read_with_callback<C>(
        &self,
        session_id: &str,
        message_seq: Option<i64>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static,
    {
        let session_id = session_id.to_string();
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = SessionApi::mark_read(self, &session_id, message_seq).await;
            match result {
                Ok(()) => callback.on_success(()),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }
}

/// 消息管理 API 回调桥接
pub trait MessageApiCallback: MessageApi {
    /// 发送消息（回调版本）
    fn send_message_with_callback<C>(
        &self,
        message: crate::domain::message::Message,
        receiver_id: Option<String>,
        channel_id: Option<String>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<String> + 'static;

    /// 获取消息列表（回调版本）
    fn get_messages_with_callback<C>(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<Vec<crate::application::vo::MessageVO>> + 'static;

    /// 撤回消息（回调版本）
    fn recall_message_with_callback<C>(
        &self,
        message_id: &str,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static;
}

impl<T: MessageApi> MessageApiCallback for T {
    fn send_message_with_callback<C>(
        &self,
        message: crate::domain::message::Message,
        receiver_id: Option<String>,
        channel_id: Option<String>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<String> + 'static,
    {
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = MessageApi::send_message(self, message, receiver_id, channel_id).await;
            match result {
                Ok(value) => callback.on_success(value),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }

    fn get_messages_with_callback<C>(
        &self,
        session_id: &str,
        limit: usize,
        cursor: Option<String>,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<Vec<crate::application::vo::MessageVO>> + 'static,
    {
        let session_id = session_id.to_string();
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = MessageApi::get_messages(self, &session_id, limit, cursor).await;
            match result {
                Ok(value) => callback.on_success(value),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }

    fn recall_message_with_callback<C>(
        &self,
        message_id: &str,
        callback: Arc<C>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>
    where
        C: Callback<()> + 'static,
    {
        let message_id = message_id.to_string();
        // 直接调用，不使用 spawn
        Box::pin(async move {
            let result = MessageApi::recall_message(self, &message_id).await;
            match result {
                Ok(()) => callback.on_success(()),
                Err(e) => {
                    let sdk_error = crate::shared::error::SDKError::from(e);
                    callback.on_error(sdk_error);
                }
            }
        })
    }
}

/// 统一回调桥接 trait
///
/// 为所有实现了 API trait 的类型自动提供回调版本
pub trait UnifiedCallbackBridge:
    ConnectionApiCallback + SessionApiCallback + MessageApiCallback
{
}

impl<T> UnifiedCallbackBridge for T where
    T: ConnectionApiCallback + SessionApiCallback + MessageApiCallback
{
}
