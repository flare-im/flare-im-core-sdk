//! 会话 ID（CID）生成与校验 — 统一委托 [flare_core::common::conversation]
//!
//! 与 Flare-Core、服务端、网关使用同一套 CID 规则，保证多端确定性一致。
//! 上层应通过本模块或 [ConversationApi] 的 `single_chat_id` / `group_id` 等方法创建会话 ID，
//! 不要自行拼接或使用非标准格式。
//!
//! # 示例
//!
//! ```
//! use flare_im_core_sdk::conversation;
//!
//! let cid = conversation::generate_single_chat_conversation_id("user_a", "user_b");
//! assert!(cid.starts_with("1A"));
//! assert_eq!(
//!     conversation::generate_single_chat_conversation_id("user_b", "user_a"),
//!     cid
//! );
//! ```

pub use flare_core::common::conversation::{
    extract_conversation_type,
    generate_ai_conversation_id,
    generate_customer_conversation_id,
    generate_group_conversation_id,
    generate_single_chat_conversation_id,
    generate_system_conversation_id,
    generate_temp_conversation_id,
    is_group_chat_conversation,
    is_single_chat_conversation,
    validate_conversation_id,
    ConversationType as CidConversationType,
};
