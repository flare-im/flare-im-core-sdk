//! 回调执行器 - 统一处理异步回调和错误
//!
//! 避免每个函数重复 spawn 和 callback 代码

use std::sync::Arc;

use crate::abi;
use crate::error_convert::{make_error, make_simple_error, make_success};
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
}

// Safety: usize 是 Send
unsafe impl Send for CallbackContext {}
unsafe impl Sync for CallbackContext {}

impl CallbackContext {
    pub fn new(user_context: *mut std::ffi::c_void, callback: FlareResultCallback) -> Self {
        Self {
            user_context: user_context as usize,
            callback,
        }
    }

    pub fn user_context_ptr(&self) -> *mut std::ffi::c_void {
        self.user_context as *mut std::ffi::c_void
    }
}

/// 执行异步操作并调用回调
///
/// # Arguments
/// * `instance` - SDK 实例
/// * `ctx` - 回调上下文
/// * `op` - 异步操作,返回 Result<T, SdkError>
/// * `to_json` - 将结果转换为 JSON 字符串
pub fn execute_async<T, F, G>(instance: Arc<SdkInstance>, ctx: CallbackContext, op: F, to_json: G)
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, flare_im_core_sdk::error::FlareError>>
        + Send
        + 'static,
    G: FnOnce(T) -> Result<String, i32> + Send + 'static,
{
    instance.runtime.spawn(async move {
        let result = op.await;

        match result {
            Ok(value) => {
                // 成功,转换为 JSON
                match to_json(value) {
                    Ok(json) => {
                        let result_json = string_to_flare(json);
                        invoke_result_callback(&ctx, make_success(), result_json);
                    }
                    Err(code) => {
                        // JSON 序列化失败
                        let error = make_simple_error(code, "Failed to serialize result");
                        invoke_result_callback(&ctx, heap_error(error), FlareString::default());
                    }
                }
            }
            Err(err) => {
                // 操作失败
                let error = make_error(&err);
                invoke_result_callback(&ctx, heap_error(error), FlareString::default());
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
pub fn execute_async_unit<F>(instance: Arc<SdkInstance>, ctx: CallbackContext, op: F)
where
    F: std::future::Future<Output = Result<(), flare_im_core_sdk::error::FlareError>>
        + Send
        + 'static,
{
    instance.runtime.spawn(async move {
        let result = op.await;

        match result {
            Ok(()) => {
                invoke_result_callback(&ctx, make_success(), FlareString::default());
            }
            Err(err) => {
                let error = make_error(&err);
                invoke_result_callback(&ctx, heap_error(error), FlareString::default());
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
    invoke_result_callback(ctx, heap_error(error), FlareString::default());
}
