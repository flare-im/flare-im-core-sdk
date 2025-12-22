//! 文本内容处理器
//!
//! 统一处理所有文本消息的内容，过滤控制字符，确保文本内容的一致性
//! 对标微信、Telegram、飞书的生产级别实现

/// 文本内容处理器
///
/// 用于统一处理文本消息的内容，包括：
/// - 过滤控制字符（如 \x0c Form Feed）
/// - 保留必要的控制字符（\n, \t, \r）
/// - 确保文本内容的一致性
pub struct TextContentProcessor;

impl TextContentProcessor {
    /// 处理文本内容，过滤控制字符
    ///
    /// # 参数
    /// * `text` - 原始文本内容
    ///
    /// # 返回
    /// 处理后的文本内容，已过滤掉不需要的控制字符
    ///
    /// # 规则
    /// - 保留所有可打印字符（ASCII 图形字符）
    /// - 保留必要的控制字符：换行符（\n）、制表符（\t）、回车符（\r）
    /// - 过滤掉其他控制字符，包括但不限于：
    ///   - \x0c (Form Feed)
    ///   - \x00 (Null)
    ///   - \x01-\x08 (其他控制字符)
    ///   - \x0b (Vertical Tab)
    ///   - \x0e-\x1f (其他控制字符)
    ///
    /// # 示例
    /// ```
    /// use flare_im_core_sdk::domain::message::text_processor::TextContentProcessor;
    ///
    /// let text = "Hello\x0cWorld\nTest";
    /// let processed = TextContentProcessor::process(text);
    /// assert_eq!(processed, "HelloWorld\nTest");
    /// ```
    pub fn process(text: impl AsRef<str>) -> String {
        text.as_ref()
            .chars()
            .filter(|c| {
                // 保留必要的控制字符：换行符、制表符、回车符
                if *c == '\n' || *c == '\t' || *c == '\r' {
                    return true;
                }
                // 过滤掉其他控制字符（包括 \x0c Form Feed）
                if c.is_control() {
                    return false;
                }
                // 保留所有非控制字符（包括字母、数字、空格、标点符号、Unicode 字符、emoji 等）
                true
            })
            .collect()
    }

    /// 处理文本内容（可变引用版本）
    ///
    /// 直接修改传入的字符串，避免不必要的内存分配
    ///
    /// # 参数
    /// * `text` - 要处理的文本内容（可变引用）
    ///
    /// # 示例
    /// ```
    /// use flare_im_core_sdk::domain::message::text_processor::TextContentProcessor;
    ///
    /// let mut text = String::from("Hello\x0cWorld\nTest");
    /// TextContentProcessor::process_in_place(&mut text);
    /// assert_eq!(text, "HelloWorld\nTest");
    /// ```
    pub fn process_in_place(text: &mut String) {
        *text = Self::process(text.as_str());
    }

    /// 检查文本是否包含需要过滤的控制字符
    ///
    /// # 参数
    /// * `text` - 要检查的文本内容
    ///
    /// # 返回
    /// 如果文本包含需要过滤的控制字符，返回 `true`
    ///
    /// # 示例
    /// ```
    /// use flare_im_core_sdk::domain::message::text_processor::TextContentProcessor;
    ///
    /// assert!(TextContentProcessor::needs_processing("Hello\x0cWorld"));
    /// assert!(!TextContentProcessor::needs_processing("Hello\nWorld"));
    /// ```
    pub fn needs_processing(text: impl AsRef<str>) -> bool {
        text.as_ref().chars().any(|c| {
            // 如果是必要的控制字符（\n, \t, \r），不需要处理
            if c == '\n' || c == '\t' || c == '\r' {
                return false;
            }
            // 如果是其他控制字符，需要处理
            c.is_control()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_filters_form_feed() {
        let text = "Hello\x0cWorld";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "HelloWorld");
    }

    #[test]
    fn test_process_preserves_newline() {
        let text = "Hello\nWorld";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "Hello\nWorld");
    }

    #[test]
    fn test_process_preserves_tab() {
        let text = "Hello\tWorld";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "Hello\tWorld");
    }

    #[test]
    fn test_process_preserves_carriage_return() {
        let text = "Hello\rWorld";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "Hello\rWorld");
    }

    #[test]
    fn test_process_filters_multiple_control_chars() {
        let text = "Hello\x00\x01\x0cWorld";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "HelloWorld");
    }

    #[test]
    fn test_process_preserves_unicode() {
        let text = "Hello 世界\x0cWorld";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "Hello 世界World");
    }

    #[test]
    fn test_process_preserves_space() {
        let text = "Hello World";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "Hello World");
    }

    #[test]
    fn test_process_preserves_chinese() {
        let text = "你好世界";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "你好世界");
    }

    #[test]
    fn test_process_preserves_emoji() {
        let text = "Hello 😀 World";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "Hello 😀 World");
    }

    #[test]
    fn test_process_filters_only_control_chars() {
        let text = "Hello\x0cWorld\x00Test";
        let processed = TextContentProcessor::process(text);
        assert_eq!(processed, "HelloWorldTest");
    }

    #[test]
    fn test_process_in_place() {
        let mut text = String::from("Hello\x0cWorld");
        TextContentProcessor::process_in_place(&mut text);
        assert_eq!(text, "HelloWorld");
    }

    #[test]
    fn test_needs_processing() {
        assert!(TextContentProcessor::needs_processing("Hello\x0cWorld"));
        assert!(!TextContentProcessor::needs_processing("Hello\nWorld"));
        assert!(!TextContentProcessor::needs_processing("Hello World"));
        assert!(!TextContentProcessor::needs_processing("你好世界"));
    }
}

