//! C ABI 包装层
//!
//! 提供 C 兼容的 API，供各平台自动生成绑定
//!
//! ## 设计原则
//!
//! 1. **最小化编码**：只做必要的类型转换
//! 2. **完全自动化**：各平台绑定自动生成
//! 3. **性能最优**：直接调用，无序列化开销
//!
//! ## 使用方式
//!
//! 1. 使用 `cbindgen` 生成 C 头文件
//! 2. 各平台从 C 头文件自动生成绑定
//! 3. 零编码，完全自动化
//!
//! ## 安全性说明
//!
//! 此模块包含 FFI 代码，虽然内部使用了 unsafe，但所有公共 API 都是安全的。
//! 所有 unsafe 操作都封装在 `safe` 模块中。

#![allow(unsafe_code)] // FFI 模块需要 unsafe，但已封装在安全包装层中

pub mod client;
pub mod error;
pub mod safe;
pub mod types;

pub use client::*;
pub use error::*;
pub use safe::*;
pub use types::*;
