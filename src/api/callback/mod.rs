//! 统一回调桥接层
//!
//! 提供跨语言友好的回调机制，将 Rust 的 `Result<T>` 转换为回调调用
//!
//! ## 模块结构
//!
//! - `core.rs`: 回调 trait 和基础实现（Callback, ClosureCallback, SplitCallback 等）
//! - `bridge.rs`: API trait 的回调桥接实现（ConnectionApiCallback, SessionApiCallback 等）
//! - `examples.rs`: 使用示例和 FFI 示例
//!
//! ## 设计原则
//!
//! 1. **统一接口**: 所有回调都遵循相同的模式（成功/失败）
//! 2. **类型安全**: 使用泛型保证类型安全
//! 3. **零开销抽象**: Rust 原生调用时直接使用 `Result<T>`，无额外开销
//! 4. **跨语言友好**: 易于通过 FFI 暴露给其他语言
//!
//! ## 使用场景
//!
//! ### Rust 原生调用（推荐）
//! ```rust,no_run
//! let result = client.login("user_123", "token").await?;
//! ```
//!
//! ### 回调调用（跨语言/异步场景）
//! ```rust,no_run
//! use flare_im_core_sdk::api::callback::*;
//!
//! let callback = callback!(|result| {
//!     match result {
//!         Ok(login_result) => println!("登录成功: {:?}", login_result),
//!         Err(e) => eprintln!("登录失败: {}", e),
//!     }
//! });
//!
//! client.login_with_callback("user_123", "token", callback).await;
//! ```
//!
//! ## 参考设计
//!
//! - **微信 SDK**: 使用 CompletionHandler 模式
//! - **Telegram SDK**: 使用 ResultCallback 模式
//! - **飞书 SDK**: 使用 Callback<T> 泛型回调
//! - **Discord SDK**: 使用 Promise/Completion 模式

pub mod bridge;
pub mod core;

#[cfg(test)]
pub mod examples;

// 重新导出核心类型和 trait
pub use bridge::*;
pub use core::*;
