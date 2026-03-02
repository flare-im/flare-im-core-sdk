//! 工具函数
//!
//! 提供消息内容提取等辅助功能


/// 从 protobuf 编码的 MessageContent 中提取文本内容
pub fn extract_text_from_protobuf_content(content: &[u8]) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    
    use flare_proto::flare::common::v1::message_content::Content;
    
    match flare_proto::decode_message_content(content) {
        Ok(msg_content) => {
            if let Some(Content::Text(text_content)) = msg_content.content {
                if !text_content.text.is_empty() {
                    return Some(text_content.text);
                }
            }
        }
        Err(e) => {
            eprintln!("[extract_text_from_protobuf_content] Failed to decode protobuf: {}, no fallback used", e);
            // 不再使用UTF-8回退，因为这会导致错误的字符显示
            return None;
        }
    }
    
    None
}

/// 确保消息的 extra.content_text 存在
pub fn ensure_message_content_text(msg: &mut flare_im_core_sdk::domain::message::Message) {
    use flare_im_core_sdk::domain::message::{ContentType, MessageType};
    
    // 只处理文本消息
    let is_text_message = matches!(msg.content_type, ContentType::PlainText) 
        || matches!(msg.message_type, MessageType::Text);
    
    if !is_text_message {
        return;
    }
    
    // 如果 extra.content_text 已存在，直接返回
    if msg.extra.contains_key("content_text") {
        return;
    }
    
    // 从 content 字段提取文本内容
    if let Some(text) = extract_text_from_protobuf_content(&msg.content) {
        msg.extra.insert("content_text".to_string(), text);
        eprintln!("[ensure_message_content_text] 手动提取并设置 content_text");
    } else {
        eprintln!("[ensure_message_content_text] 无法从 content 提取文本内容");
    }
}


