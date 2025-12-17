//! 回调核心实现
//!
//! 定义回调 trait 和基础实现

use std::sync::Arc;
use tracing::error;

/// 统一回调 trait
///
/// 所有异步操作都可以通过此回调接收结果
///
/// ## 设计说明
///
/// 1. **Send + Sync**: 保证可以在多线程环境中使用
/// 2. **'static**: 回调生命周期独立，不依赖调用者
/// 3. **泛型 T**: 成功时的返回类型
///
/// ## 使用示例
///
/// ```rust,no_run
/// let callback = Arc::new(MyCallback::new());
/// client.login_with_callback("user_123", "token", callback).await;
/// ```
pub trait Callback<T>: Send + Sync {
    /// 调用回调（成功）
    fn on_success(&self, result: T);

    /// 调用回调（失败）
    fn on_error(&self, error: crate::shared::error::SDKError);
}

/// 闭包回调实现
///
/// 允许使用闭包快速创建回调
///
/// ## 示例
///
/// ```rust,no_run
/// let callback = ClosureCallback::new(|result| {
///     match result {
///         Ok(value) => println!("成功: {:?}", value),
///         Err(e) => eprintln!("失败: {}", e),
///     }
/// });
/// ```
pub struct ClosureCallback<T, F>
where
    F: Fn(Result<T, crate::shared::error::SDKError>) + Send + Sync + 'static,
{
    closure: Arc<F>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F> ClosureCallback<T, F>
where
    F: Fn(Result<T, crate::shared::error::SDKError>) + Send + Sync + 'static,
{
    /// 创建闭包回调
    pub fn new(closure: F) -> Self {
        Self {
            closure: Arc::new(closure),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, F> Callback<T> for ClosureCallback<T, F>
where
    T: Send + Sync,
    F: Fn(Result<T, crate::shared::error::SDKError>) + Send + Sync + 'static,
{
    fn on_success(&self, result: T) {
        (self.closure)(Ok(result));
    }

    fn on_error(&self, error: crate::shared::error::SDKError) {
        (self.closure)(Err(error));
    }
}

/// 分离的成功/失败回调
///
/// 某些场景下，分离的成功和失败回调更清晰
///
/// ## 示例
///
/// ```rust,no_run
/// let callback = SplitCallback::new(
///     |result| println!("成功: {:?}", result),
///     |error| eprintln!("失败: {}", error),
/// );
/// ```
pub struct SplitCallback<T> {
    on_success: Arc<dyn Fn(T) + Send + Sync + 'static>,
    on_error: Arc<dyn Fn(crate::shared::error::SDKError) + Send + Sync + 'static>,
}

impl<T> SplitCallback<T> {
    /// 创建分离回调
    pub fn new<F1, F2>(on_success: F1, on_error: F2) -> Self
    where
        F1: Fn(T) + Send + Sync + 'static,
        F2: Fn(crate::shared::error::SDKError) + Send + Sync + 'static,
    {
        Self {
            on_success: Arc::new(on_success),
            on_error: Arc::new(on_error),
        }
    }
}

impl<T> Callback<T> for SplitCallback<T>
where
    T: Send + Sync,
{
    fn on_success(&self, result: T) {
        (self.on_success)(result);
    }

    fn on_error(&self, error: crate::shared::error::SDKError) {
        (self.on_error)(error);
    }
}

/// 空回调（用于不需要处理结果的场景）
///
/// ## 使用场景
///
/// - 日志记录
/// - 性能测试
/// - 忽略结果的操作
pub struct NoOpCallback<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> NoOpCallback<T> {
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T> Default for NoOpCallback<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Callback<T> for NoOpCallback<T>
where
    T: Send + Sync,
{
    fn on_success(&self, _result: T) {
        // 空实现
    }

    fn on_error(&self, error: crate::shared::error::SDKError) {
        // 记录错误日志，但不做其他处理
        error!(error = %error, "NoOpCallback: Operation failed");
    }
}

/// 回调桥接工具
///
/// 提供将 `Result<T>` 转换为回调调用的工具函数
pub struct CallbackBridge;

impl CallbackBridge {
    /// 执行异步操作并调用回调
    ///
    /// ## 参数
    ///
    /// - `future`: 异步操作（返回 `Result<T>`）
    /// - `callback`: 回调对象
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// CallbackBridge::execute(
    ///     client.login("user_123", "token"),
    ///     callback,
    /// ).await;
    /// ```
    pub async fn execute<T, C, F>(future: F, callback: Arc<C>)
    where
        T: Send + 'static,
        C: Callback<T>,
        F: std::future::Future<Output = anyhow::Result<T>> + Send,
    {
        match future.await {
            Ok(value) => callback.on_success(value),
            Err(e) => {
                let sdk_error = crate::shared::error::SDKError::from(e);
                callback.on_error(sdk_error);
            }
        }
    }

    /// 执行异步操作并调用回调（带上下文）
    ///
    /// 允许在回调中传递额外的上下文信息
    pub async fn execute_with_context<T, C, CTX>(
        future: impl std::future::Future<Output = anyhow::Result<T>>,
        callback: Arc<C>,
        context: CTX,
    ) where
        T: Send + 'static,
        C: Callback<(T, CTX)>,
        CTX: Send + 'static,
    {
        match future.await {
            Ok(value) => callback.on_success((value, context)),
            Err(e) => {
                let sdk_error = crate::shared::error::SDKError::from(e);
                callback.on_error(sdk_error);
            }
        }
    }

    /// 执行同步操作并调用回调
    ///
    /// 用于同步操作的回调调用
    pub fn execute_sync<T, C>(operation: impl FnOnce() -> anyhow::Result<T>, callback: Arc<C>)
    where
        T: Send + 'static,
        C: Callback<T>,
    {
        match operation() {
            Ok(value) => callback.on_success(value),
            Err(e) => {
                let sdk_error = crate::shared::error::SDKError::from(e);
                callback.on_error(sdk_error);
            }
        }
    }
}

/// 便捷宏：创建闭包回调
///
/// ## 示例
///
/// ```rust,no_run
/// let callback = callback!(|result| {
///     match result {
///         Ok(v) => println!("成功: {:?}", v),
///         Err(e) => eprintln!("失败: {}", e),
///     }
/// });
/// ```
#[macro_export]
macro_rules! callback {
    ($closure:expr) => {
        Arc::new($crate::api::callback::ClosureCallback::new($closure))
    };
}

/// 便捷宏：创建分离回调
///
/// ## 示例
///
/// ```rust,no_run
/// let callback = split_callback!(
///     |result| println!("成功: {:?}", result),
///     |error| eprintln!("失败: {}", error),
/// );
/// ```
#[macro_export]
macro_rules! split_callback {
    ($on_success:expr, $on_error:expr) => {
        Arc::new($crate::api::callback::SplitCallback::new(
            $on_success,
            $on_error,
        ))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn test_closure_callback() {
        let success_called = Arc::new(AtomicBool::new(false));
        let error_called = Arc::new(AtomicBool::new(false));

        let success_flag = Arc::clone(&success_called);
        let error_flag = Arc::clone(&error_called);

        let callback = ClosureCallback::new(move |result: Result<String, _>| match result {
            Ok(_) => success_flag.store(true, Ordering::Relaxed),
            Err(_) => error_flag.store(true, Ordering::Relaxed),
        });

        // 测试成功回调
        callback.on_success("test".to_string());
        assert!(success_called.load(Ordering::Relaxed));
        assert!(!error_called.load(Ordering::Relaxed));

        // 重置
        success_called.store(false, Ordering::Relaxed);
        error_called.store(false, Ordering::Relaxed);

        // 测试失败回调
        let error = crate::shared::error::SDKError::Wrapped {
            message: "test error".to_string(),
        };
        callback.on_error(error);
        assert!(!success_called.load(Ordering::Relaxed));
        assert!(error_called.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_split_callback() {
        let success_value = Arc::new(std::sync::Mutex::new(None::<String>));
        let error_value = Arc::new(std::sync::Mutex::new(None::<String>));

        let success_val = Arc::clone(&success_value);
        let error_val = Arc::clone(&error_value);

        let callback = SplitCallback::new(
            move |result: String| {
                *success_val.lock().unwrap() = Some(result);
            },
            move |error: crate::shared::error::SDKError| {
                *error_val.lock().unwrap() = Some(error.to_string());
            },
        );

        // 测试成功回调
        callback.on_success("test".to_string());
        assert_eq!(
            success_value.lock().unwrap().as_ref(),
            Some(&"test".to_string())
        );

        // 测试失败回调
        let error = crate::shared::error::SDKError::Wrapped {
            message: "test error".to_string(),
        };
        callback.on_error(error);
        assert!(error_value.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn test_callback_bridge() {
        let success_called = Arc::new(AtomicBool::new(false));

        let success_flag = Arc::clone(&success_called);
        let callback = ClosureCallback::new(move |result: Result<String, _>| {
            if result.is_ok() {
                success_flag.store(true, Ordering::Relaxed);
            }
        });

        // 测试成功场景
        CallbackBridge::execute(async { Ok("test".to_string()) }, Arc::new(callback)).await;

        assert!(success_called.load(Ordering::Relaxed));
    }

    #[test]
    fn test_noop_callback() {
        let callback = NoOpCallback::<String>::new();
        // 应该不会 panic
        callback.on_success("test".to_string());
        callback.on_error(crate::shared::error::SDKError::Wrapped {
            message: "test".to_string(),
        });
    }
}
