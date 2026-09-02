//! 提及解析：从正文里判定 `@全员` 与 `@某人`。
//!
//! 这是**产品中立的内容规则**——同一段文字在任何端都必须产出同一条消息，
//! 所以规则只能有一份，放在核心（flare-im-spec 约束 4）。
//!
//! 此前这条规则只存在于 Vue kit 的一行正则 `/(^|\s)@all(\s|$)/i` 里：
//! 中文用户打 `@全员`/`@所有人` 不触发，而 Flutter / iOS / Android 压根不解析，
//! `mentionAll` 恒为 false —— 同样的文本在不同端产生不同的消息。

/// 一个可被提及的人：用户 id + 可能的显示名（昵称、备注等）。
#[derive(Debug, Clone)]
pub struct MentionCandidate {
    pub user_id: String,
    pub display_names: Vec<String>,
}

/// 解析结果。`user_ids` 按正文出现顺序去重。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedMentions {
    pub mention_all: bool,
    pub user_ids: Vec<String>,
}

/// 触发「@全员」的记号。大小写不敏感（仅对 ASCII 有意义）。
const MENTION_ALL_TOKENS: [&str; 4] = ["all", "everyone", "全员", "所有人"];

/// `@` 是否处在一个合法的提及起始位置。
///
/// 要求前一个字符是空白、或本身就是开头，否则 `foo@all.com` 这类邮箱会被误判。
/// 但中文输入普遍不加空格（`你好@全员`），所以额外放行「前一个字符是 CJK」——
/// 邮箱的本地部分不会以中日韩文字结尾，两边都能照顾到。
fn is_mention_boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_whitespace() || is_cjk(c),
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF      // 扩展 A
        | 0x4E00..=0x9FFF    // 基本区
        | 0xF900..=0xFAFF    // 兼容表意
        | 0x3000..=0x303F    // 中日韩符号与标点
        | 0xFF00..=0xFFEF    // 全角
    )
}

/// 提及记号允许的字符：ASCII 字母数字与 `_ . -`，以及 CJK。
/// 空白与常见标点天然成为终止符。
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') || is_cjk(c) && !is_cjk_punctuation(c)
}

fn is_cjk_punctuation(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x303F | 0xFF00..=0xFF0F | 0xFF1A..=0xFF20)
}

/// 从正文解析提及。
///
/// 匹配采用**最长优先**：`@张三丰` 不会被 `@张三` 抢走。
pub fn parse_mentions(text: &str, candidates: &[MentionCandidate]) -> ParsedMentions {
    let chars: Vec<char> = text.chars().collect();
    let mut result = ParsedMentions::default();
    let mut seen: Vec<String> = Vec::new();

    // 候选记号表：记号 -> Some(user_id) 表示提及某人，None 表示 @全员。
    let mut tokens: Vec<(String, Option<String>)> = Vec::new();
    for token in MENTION_ALL_TOKENS {
        tokens.push((token.to_string(), None));
    }
    for candidate in candidates {
        let id = candidate.user_id.trim();
        if !id.is_empty() {
            tokens.push((id.to_string(), Some(id.to_string())));
        }
        for name in &candidate.display_names {
            let name = name.trim();
            if !name.is_empty() {
                tokens.push((name.to_string(), Some(id.to_string())));
            }
        }
    }
    // 长记号优先，避免前缀吃掉更长的匹配。
    tokens.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));

    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        if !is_mention_boundary(prev) {
            i += 1;
            continue;
        }
        // 取出 `@` 之后的记号（到第一个非记号字符为止）。
        let mut end = i + 1;
        while end < chars.len() && is_token_char(chars[end]) {
            end += 1;
        }
        let raw: String = chars[i + 1..end].iter().collect();
        if raw.is_empty() {
            i += 1;
            continue;
        }

        let mut matched_len = 0usize;
        for (token, target) in &tokens {
            if token.chars().count() > raw.chars().count() {
                continue;
            }
            let head: String = raw.chars().take(token.chars().count()).collect();
            if !head.eq_ignore_ascii_case(token) {
                continue;
            }
            match target {
                None => result.mention_all = true,
                Some(user_id) => {
                    if !seen.iter().any(|id| id == user_id) {
                        seen.push(user_id.clone());
                    }
                }
            }
            matched_len = token.chars().count();
            break;
        }

        i = if matched_len > 0 { i + 1 + matched_len } else { end.max(i + 1) };
    }

    result.user_ids = seen;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<MentionCandidate> {
        vec![
            MentionCandidate {
                user_id: "webtest2".to_string(),
                display_names: vec!["张三".to_string()],
            },
            MentionCandidate {
                user_id: "u2".to_string(),
                display_names: vec!["张三丰".to_string()],
            },
        ]
    }

    #[test]
    fn chinese_at_all_triggers_mention_all() {
        // 这条是本次修复的起点：中文用户打 @全员 从来没生效过。
        for text in ["@全员 开会了", "你好@所有人", "@全员", "开会@全员！"] {
            assert!(
                parse_mentions(text, &candidates()).mention_all,
                "{text} 应当触发 @全员"
            );
        }
    }

    #[test]
    fn ascii_at_all_still_works() {
        assert!(parse_mentions("@all hello", &candidates()).mention_all);
        assert!(parse_mentions("hi @everyone", &candidates()).mention_all);
        assert!(parse_mentions("@ALL", &candidates()).mention_all);
    }

    #[test]
    fn email_like_text_is_not_a_mention() {
        // 前一个字符不是空白也不是 CJK → 不是提及边界。
        let parsed = parse_mentions("联系 foo@all.com", &candidates());
        assert!(!parsed.mention_all, "邮箱不能被误判成 @全员");
    }

    #[test]
    fn mentions_by_user_id_and_display_name() {
        assert_eq!(
            parse_mentions("@webtest2 在吗", &candidates()).user_ids,
            vec!["webtest2".to_string()]
        );
        assert_eq!(
            parse_mentions("@张三 在吗", &candidates()).user_ids,
            vec!["webtest2".to_string()],
            "中文显示名必须能被提及——旧的 ASCII-only 正则做不到"
        );
    }

    #[test]
    fn longest_candidate_wins() {
        // @张三丰 不能被 @张三 抢走，否则提及到错误的人。
        assert_eq!(
            parse_mentions("@张三丰 你好", &candidates()).user_ids,
            vec!["u2".to_string()]
        );
    }

    #[test]
    fn non_member_mention_is_ignored() {
        assert!(parse_mentions("@nobody 在吗", &candidates()).user_ids.is_empty());
    }

    #[test]
    fn duplicate_mentions_are_deduped_in_order() {
        let parsed = parse_mentions("@张三 @u2 @webtest2", &candidates());
        assert_eq!(parsed.user_ids, vec!["webtest2".to_string(), "u2".to_string()]);
    }
}
