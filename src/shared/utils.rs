//! 工具模块
//!
//! 提供常用工具函数，包括会话 ID 生成等

// 重新导出 flare-core 的会话 ID 生成函数，方便 SDK 内部使用
// 注意：flare-core 已经通过 lib.rs 重新导出了这些函数
pub use flare_core::{
    generate_single_chat_conversation_id,
    generate_group_conversation_id,
    generate_ai_conversation_id,
    generate_customer_conversation_id,
    generate_system_conversation_id,
    generate_temp_conversation_id,
    validate_conversation_id,
    extract_conversation_type,
    is_single_chat_conversation,
    is_group_chat_conversation,
    ConversationType,
};
