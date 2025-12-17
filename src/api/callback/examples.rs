//! 回调桥接层使用示例
//!
//! 展示如何在 Rust 和跨语言场景中使用回调机制

#[cfg(test)]
mod examples {
    use crate::api::callback::*;
    use crate::api::traits::*;
    use std::sync::Arc;

    /// 示例 1: Rust 原生调用（推荐）
    ///
    /// 直接使用 `Result<T>`，零开销
    #[tokio::test]
    #[ignore] // 需要实际的 client 实例
    async fn example_rust_native() {
        // let client = FlareIMClient::new(config).await?;
        //
        // // 直接使用 Result，最简洁
        // let login_result = client.login("user_123", "token").await?;
        // println!("登录成功: {:?}", login_result);
        //
        // // 发送消息
        // let msg_id = client.send_message(message, None, None).await?;
        // println!("消息已发送: {}", msg_id);
    }

    /// 示例 2: 使用闭包回调
    ///
    /// 适合需要统一处理成功/失败的场景
    #[tokio::test]
    #[ignore]
    async fn example_closure_callback() {
        // let client = FlareIMClient::new(config).await?;
        //
        // // 使用闭包回调
        // let callback = callback!(|result: Result<LoginResult, _>| {
        //     match result {
        //         Ok(login_result) => {
        //             println!("登录成功: {:?}", login_result);
        //             // 可以在这里触发后续操作
        //         }
        //         Err(e) => {
        //             eprintln!("登录失败: {}", e);
        //             // 可以在这里处理错误恢复
        //         }
        //     }
        // });
        //
        // client.login_with_callback("user_123", "token", callback).await;
    }

    /// 示例 3: 使用分离回调
    ///
    /// 适合成功和失败处理逻辑分离的场景
    #[tokio::test]
    #[ignore]
    async fn example_split_callback() {
        // let client = FlareIMClient::new(config).await?;
        //
        // let callback = split_callback!(
        //     |login_result: LoginResult| {
        //         println!("登录成功: {:?}", login_result);
        //         // 成功处理逻辑
        //     },
        //     |error: SDKError| {
        //         eprintln!("登录失败: {}", error);
        //         // 错误处理逻辑（如重试、降级等）
        //     }
        // );
        //
        // client.login_with_callback("user_123", "token", callback).await;
    }

    /// 示例 4: 使用 CallbackBridge 手动桥接
    ///
    /// 适合需要自定义桥接逻辑的场景
    #[tokio::test]
    #[ignore]
    async fn example_manual_bridge() {
        // let client = FlareIMClient::new(config).await?;
        //
        // let callback = callback!(|result: Result<String, _>| {
        //     match result {
        //         Ok(msg_id) => println!("消息已发送: {}", msg_id),
        //         Err(e) => eprintln!("发送失败: {}", e),
        //     }
        // });
        //
        // // 手动桥接
        // CallbackBridge::execute(
        //     client.send_message(message, None, None),
        //     callback,
        // ).await;
    }

    /// 示例 5: 带上下文的回调
    ///
    /// 适合需要在回调中访问额外上下文的场景
    #[tokio::test]
    #[ignore]
    async fn example_callback_with_context() {
        // let client = FlareIMClient::new(config).await?;
        //
        // struct Context {
        //     session_id: String,
        //     retry_count: u32,
        // }
        //
        // let context = Context {
        //     session_id: "session_123".to_string(),
        //     retry_count: 0,
        // };
        //
        // let callback = callback!(|result: Result<(String, Context), _>| {
        //     match result {
        //         Ok((msg_id, ctx)) => {
        //             println!("消息 {} 已发送到会话 {}", msg_id, ctx.session_id);
        //         }
        //         Err(e) => {
        //             eprintln!("发送失败: {}", e);
        //             // 可以使用 ctx.retry_count 进行重试
        //         }
        //     }
        // });
        //
        // CallbackBridge::execute_with_context(
        //     client.send_message(message, None, None),
        //     callback,
        //     context,
        // ).await;
    }

    /// 示例 6: 批量操作回调
    ///
    /// 适合需要批量处理多个异步操作的场景
    #[tokio::test]
    #[ignore]
    async fn example_batch_operations() {
        // let client = FlareIMClient::new(config).await?;
        //
        // use std::sync::atomic::{AtomicU32, Ordering};
        // let success_count = Arc::new(AtomicU32::new(0));
        // let error_count = Arc::new(AtomicU32::new(0));
        //
        // let success_cnt = Arc::clone(&success_count);
        // let error_cnt = Arc::clone(&error_count);
        //
        // let callback = callback!(move |result: Result<String, _>| {
        //     match result {
        //         Ok(_) => success_cnt.fetch_add(1, Ordering::Relaxed),
        //         Err(_) => error_cnt.fetch_add(1, Ordering::Relaxed),
        //     }
        // });
        //
        // // 批量发送消息
        // for message in messages {
        //     client.send_message_with_callback(message, None, None, Arc::clone(&callback)).await;
        // }
        //
        // // 等待所有操作完成（实际场景中需要使用更复杂的同步机制）
        // tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        //
        // println!("成功: {}, 失败: {}",
        //     success_count.load(Ordering::Relaxed),
        //     error_count.load(Ordering::Relaxed)
        // );
    }

    /// 示例 7: 链式回调
    ///
    /// 适合需要链式调用多个异步操作的场景
    #[tokio::test]
    #[ignore]
    async fn example_chained_callbacks() {
        // let client = FlareIMClient::new(config).await?;
        //
        // // 第一步：登录
        // let login_callback = callback!(move |result: Result<LoginResult, _>| {
        //     match result {
        //         Ok(_) => {
        //             println!("登录成功，开始同步会话");
        //             // 第二步：同步会话
        //             let sync_callback = callback!(|result: Result<Vec<SessionSummary>, _>| {
        //                 match result {
        //                     Ok(sessions) => {
        //                         println!("同步成功，获得 {} 个会话", sessions.len());
        //                         // 可以继续链式调用
        //                     }
        //                     Err(e) => eprintln!("同步失败: {}", e),
        //                 }
        //             });
        //             // client.sync_sessions_with_callback(sync_callback).await;
        //         }
        //         Err(e) => eprintln!("登录失败: {}", e),
        //     }
        // });
        //
        // client.login_with_callback("user_123", "token", login_callback).await;
    }
}

/// FFI 使用示例（伪代码）
///
/// 展示如何通过 FFI 暴露回调给其他语言
///
/// ## C/Objective-C (iOS/macOS)
///
/// ```c
/// // C 回调函数类型
/// typedef void (*FlareIMCallback)(void* result, void* error);
///
/// // C API
/// void flare_im_login(
///     FlareIMClientHandle client,
///     const char* user_id,
///     const char* token,
///     FlareIMCallback callback
/// );
/// ```
///
/// ## Java (Android)
///
/// ```java
/// // Java 回调接口
/// public interface FlareIMCallback<T> {
///     void onSuccess(T result);
///     void onError(SDKError error);
/// }
///
/// // Java API
/// public void login(String userId, String token, FlareIMCallback<LoginResult> callback);
/// ```
///
/// ## JavaScript/TypeScript (Web)
///
/// ```typescript
/// // TypeScript 回调类型
/// type FlareIMCallback<T> = (result: Result<T, SDKError>) => void;
///
/// // TypeScript API
/// async function login(
///     userId: string,
///     token: string,
///     callback: FlareIMCallback<LoginResult>
/// ): Promise<void>;
///
/// // 使用示例
/// await client.login("user_123", "token", (result) => {
///     if (result.isOk()) {
///         console.log("登录成功:", result.value);
///     } else {
///         console.error("登录失败:", result.error);
///     }
/// });
/// ```
///
/// ## Dart (Flutter)
///
/// ```dart
/// // Dart 回调类型
/// typedef FlareIMCallback<T> = void Function(Result<T, SDKError> result);
///
/// // Dart API
/// Future<void> login(
///     String userId,
///     String token,
///     FlareIMCallback<LoginResult> callback,
/// );
///
/// // 使用示例
/// await client.login("user_123", "token", (result) {
///     result.when(
///         success: (value) => print("登录成功: $value"),
///         failure: (error) => print("登录失败: $error"),
///     );
/// });
/// ```
pub mod ffi_examples {}
