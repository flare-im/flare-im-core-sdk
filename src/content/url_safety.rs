//! URL scheme rules shared by structured message content validators.

const ALLOWED_LINK_SCHEMES: &[&str] = &["http", "https", "mailto", "tel"];
const ALLOWED_REMOTE_URL_SCHEMES: &[&str] = &["http", "https"];

pub fn is_safe_link_href(href: &str) -> bool {
    match classify_scheme(href) {
        SchemeClass::Scheme(scheme) => ALLOWED_LINK_SCHEMES.contains(&scheme.as_str()),
        SchemeClass::SchemeLess => true,
        SchemeClass::Invalid => false,
    }
}

pub fn is_safe_remote_url(url: &str) -> bool {
    match classify_scheme(url) {
        SchemeClass::Scheme(scheme) => ALLOWED_REMOTE_URL_SCHEMES.contains(&scheme.as_str()),
        SchemeClass::SchemeLess | SchemeClass::Invalid => false,
    }
}

enum SchemeClass {
    Scheme(String),
    SchemeLess,
    Invalid,
}

fn classify_scheme(input: &str) -> SchemeClass {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    if cleaned.is_empty() {
        return SchemeClass::Invalid;
    }

    let mut scheme = String::new();
    for c in cleaned.chars() {
        match c {
            ':' => {
                if scheme.is_empty() {
                    return SchemeClass::Invalid;
                }
                return SchemeClass::Scheme(scheme.to_ascii_lowercase());
            }
            '/' | '?' | '#' => return SchemeClass::SchemeLess,
            c if c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-') => scheme.push(c),
            _ => return SchemeClass::SchemeLess,
        }
    }
    SchemeClass::SchemeLess
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_href_allows_safe_schemes_and_relative_values() {
        for href in [
            "https://flare.test",
            "HTTP://flare.test",
            "mailto:team@flare.test",
            "tel:+10000000000",
            "/relative/path",
            "?query=1",
            "#anchor",
        ] {
            assert!(is_safe_link_href(href), "{href}");
        }
    }

    #[test]
    fn link_href_rejects_executable_schemes_after_browser_cleanup() {
        for href in [
            "javascript:alert(1)",
            "java\nscript:alert(1)",
            "\u{1}javascript:alert(1)",
            "data:text/html,<script></script>",
            "vbscript:msgbox(1)",
            "file:///tmp/a",
        ] {
            assert!(!is_safe_link_href(href), "{href}");
        }
    }

    #[test]
    fn remote_url_allows_only_http_and_https() {
        assert!(is_safe_remote_url("https://flare.test/a.png"));
        assert!(is_safe_remote_url("HTTP://flare.test/a.png"));
        assert!(!is_safe_remote_url("/relative.png"));
        assert!(!is_safe_remote_url("mailto:team@flare.test"));
        assert!(!is_safe_remote_url("javascript:alert(1)"));
    }
}
