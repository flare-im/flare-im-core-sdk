//! 回调执行器 - 统一处理异步回调和错误
//!
//! 避免每个函数重复 spawn 和 callback 代码

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::abi;
use crate::error_convert::{FLARE_ERR_FFI_PANIC, make_error, make_simple_error, make_success};
use crate::helpers::string_to_flare;
use crate::registry::SdkInstance;
use crate::types::{FlareError, FlareResultCallback, FlareString};

/// 供 Dart `NativeCallable.listener` 等跨线程回调使用：`error` 非空时须为 [`Box::into_raw`]，由 Dart 侧调用 `flare_error_heap_free`。
#[inline]
fn invoke_result_callback(ctx: &CallbackContext, error: *const FlareError, result: FlareString) {
    abi::invoke_user_c_callback("FlareResultCallback", || {
        (ctx.callback)(ctx.user_context_ptr(), error, result);
    });
}

#[inline]
fn heap_error(e: FlareError) -> *const FlareError {
    Box::into_raw(Box::new(e))
}

/// 回调上下文 - 包装用户上下文和回调函数
pub struct CallbackContext {
    pub user_context: usize, // 使用 usize 代替 *mut c_void 以确保 Send
    pub callback: FlareResultCallback,
    fired: AtomicBool,
    enabled: Option<Arc<AtomicBool>>,
}

// Safety: user_context 存为 usize，callback 为 C ABI 函数指针，完成标记为原子值。
unsafe impl Send for CallbackContext {}
unsafe impl Sync for CallbackContext {}

impl CallbackContext {
    pub fn new(user_context: *mut std::ffi::c_void, callback: FlareResultCallback) -> Self {
        Self {
            user_context: user_context as usize,
            callback,
            fired: AtomicBool::new(false),
            enabled: None,
        }
    }

    pub fn user_context_ptr(&self) -> *mut std::ffi::c_void {
        self.user_context as *mut std::ffi::c_void
    }

    fn complete(&self, error: *const FlareError, result: FlareString) {
        if self
            .enabled
            .as_ref()
            .is_some_and(|enabled| !enabled.load(Ordering::Acquire))
        {
            self.fired.store(true, Ordering::Release);
            crate::helpers::flare_string_free(result);
            if !error.is_null() {
                // Errors passed to complete are always owned heap_error allocations.
                unsafe {
                    crate::helpers::flare_error_heap_free(error as *mut FlareError);
                }
            }
            return;
        }
        if self
            .fired
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            invoke_result_callback(self, error, result);
        }
    }
}

impl Drop for CallbackContext {
    fn drop(&mut self) {
        if self.fired.swap(true, Ordering::AcqRel)
            || self
                .enabled
                .as_ref()
                .is_some_and(|enabled| !enabled.load(Ordering::Acquire))
        {
            return;
        }
        let error = make_simple_error(
            FLARE_ERR_FFI_PANIC,
            "FFI async operation dropped before callback completion",
        );
        invoke_result_callback(self, heap_error(error), FlareString::default());
    }
}

/// 执行异步操作并调用回调
///
/// # Arguments
/// * `instance` - SDK 实例
/// * `ctx` - 回调上下文
/// * `op` - 异步操作,返回 Result<T, SdkError>
/// * `to_json` - 将结果转换为 JSON 字符串
pub fn execute_async<T, F, G>(
    instance: Arc<SdkInstance>,
    operation: &str,
    mut ctx: CallbackContext,
    op: F,
    to_json: G,
) where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, flare_im_core_sdk::FlareError>> + Send + 'static,
    G: FnOnce(T) -> Result<String, i32> + Send + 'static,
{
    ctx.enabled = Some(instance.callbacks_enabled.clone());
    let mut invocation = instance.invocations.begin(operation);
    let cancellable = invocation.is_cancellable();
    let generation = instance.client.session_generation_snapshot();
    let runtime = instance.runtime.clone();
    runtime.spawn(async move {
        let result = if cancellable {
            tokio::select! {
                biased;
                _ = invocation.cancelled() => Err(flare_im_core_sdk::FlareError::localized(
                    flare_im_core_sdk::ErrorCode::NotConnected, "SDK instance released; operation cancelled",
                )),
                result = tokio::time::timeout(std::time::Duration::from_secs(120), op) => {
                    result.unwrap_or_else(|_| Err(flare_im_core_sdk::FlareError::localized(
                        flare_im_core_sdk::ErrorCode::OperationTimeout,
                        "Native operation timed out; remote writes may already have committed",
                    )))
                }
            }
        } else { op.await };
        if !instance.callbacks_enabled.load(Ordering::Acquire) {
            ctx.fired.store(true, Ordering::Release);
            return;
        }
        let result = if cancellable && generation != instance.client.session_generation_snapshot() {
            Err(flare_im_core_sdk::FlareError::localized(
                flare_im_core_sdk::ErrorCode::NotConnected, "Session changed before operation completed",
            ))
        } else { result };

        match result {
            Ok(value) => {
                // 成功,转换为 JSON
                match to_json(value) {
                    Ok(json) => {
                        let result_json = string_to_flare(json);
                        ctx.complete(make_success(), result_json);
                    }
                    Err(code) => {
                        // JSON 序列化失败
                        let error = make_simple_error(code, "Failed to serialize result");
                        ctx.complete(heap_error(error), FlareString::default());
                    }
                }
            }
            Err(err) => {
                // 操作失败
                let error = make_error(&err);
                ctx.complete(heap_error(error), FlareString::default());
            }
        }
    });
}

/// 执行异步操作(无返回值)并调用回调
///
/// # Arguments
/// * `instance` - SDK 实例
/// * `ctx` - 回调上下文
/// * `op` - 异步操作,返回 Result<(), SdkError>
pub fn execute_async_unit<F>(instance: Arc<SdkInstance>, mut ctx: CallbackContext, op: F)
where
    F: std::future::Future<Output = Result<(), flare_im_core_sdk::FlareError>> + Send + 'static,
{
    ctx.enabled = Some(instance.callbacks_enabled.clone());
    let runtime = instance.runtime.clone();
    runtime.spawn(async move {
        let result = op.await;
        if !instance.callbacks_enabled.load(Ordering::Acquire) {
            ctx.fired.store(true, Ordering::Release);
            return;
        }

        match result {
            Ok(()) => {
                ctx.complete(make_success(), FlareString::default());
            }
            Err(err) => {
                let error = make_error(&err);
                ctx.complete(heap_error(error), FlareString::default());
            }
        }
    });
}

/// 立即返回错误(同步)
///
/// 用于参数验证失败等场景
#[inline]
pub fn return_error(ctx: &CallbackContext, code: i32, message: &str) {
    let error = make_simple_error(code, message);
    ctx.complete(heap_error(error), FlareString::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{release_instance, require_instance};

    extern "C" fn collect(
        context: *mut std::ffi::c_void,
        error: *const FlareError,
        result: FlareString,
    ) {
        // Test owns the sender until the single completion has arrived.
        let sender = unsafe { &*(context as *const tokio::sync::mpsc::UnboundedSender<i32>) };
        let code = if error.is_null() {
            0
        } else {
            unsafe { (*error).code }
        };
        let _ = sender.send(code);
        crate::helpers::flare_string_free(result);
        if !error.is_null() {
            unsafe {
                crate::helpers::flare_error_heap_free(error as *mut FlareError);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn native_timeout_completes_once_without_replacing_instance() {
        let handle = crate::registry::register_instance(Arc::new(SdkInstance {
            client: flare_im_core_sdk::client::IMClient::new(),
            runtime: tokio::runtime::Handle::current(),
            im_session: Default::default(),
            invocations: Default::default(),
            callbacks_enabled: Arc::new(AtomicBool::new(true)),
        }));
        let instance = require_instance(handle).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<i32>();
        let mut sender = Box::new(sender);
        let ctx = CallbackContext::new((&mut *sender as *mut _) as *mut _, collect);
        execute_async(
            instance.clone(),
            "message.search",
            ctx,
            std::future::pending::<Result<(), flare_im_core_sdk::FlareError>>(),
            |_| Ok("{}".into()),
        );
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(121)).await;
        assert_eq!(
            receiver.recv().await,
            Some(crate::error_convert::FLARE_ERR_OPERATION_TIMEOUT)
        );
        assert!(Arc::ptr_eq(&instance, &require_instance(handle).unwrap()));
        assert!(receiver.try_recv().is_err());
        release_instance(handle);
    }

    #[tokio::test]
    async fn release_cancels_waiting_native_query() {
        let handle = crate::lifecycle::flare_sdk_create();
        let instance = require_instance(handle).unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<i32>();
        let mut sender = Box::new(sender);
        let ctx = CallbackContext::new((&mut *sender as *mut _) as *mut _, collect);
        execute_async(
            instance,
            "message.search",
            ctx,
            std::future::pending::<Result<(), flare_im_core_sdk::FlareError>>(),
            |_| Ok("{}".into()),
        );
        release_instance(handle);
        assert_eq!(
            receiver.recv().await,
            Some(crate::error_convert::FLARE_ERR_NOT_CONNECTED)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn hard_reset_gate_suppresses_dropped_context_callback() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<i32>();
        let mut sender = Box::new(sender);
        let mut ctx = CallbackContext::new((&mut *sender as *mut _) as *mut _, collect);
        ctx.enabled = Some(Arc::new(AtomicBool::new(false)));
        drop(ctx);
        assert!(receiver.try_recv().is_err());
    }
}
