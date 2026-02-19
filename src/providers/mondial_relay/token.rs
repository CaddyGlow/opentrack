use std::sync::LazyLock;

use regex::Regex;

pub(crate) const REQUEST_VERIFICATION_TOKEN_MARKER: &str = "__RequestVerificationToken";
pub(crate) const REQUEST_VERIFICATION_TOKEN_SELECTOR: &str =
    r#"input[name="__RequestVerificationToken"]"#;
pub(crate) const REQUEST_VERIFICATION_TOKEN_PRESENCE_JS: &str =
    r#"Boolean(document.querySelector('input[name="__RequestVerificationToken"]')?.value)"#;

static TOKEN_REGEXES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r#"name="__RequestVerificationToken"[^>]*value="([^"]+)""#,
        r#"value="([^"]+)"[^>]*name="__RequestVerificationToken""#,
    ]
    .iter()
    .filter_map(|pattern| Regex::new(pattern).ok())
    .collect()
});

pub(crate) fn extract_request_verification_token(html: &str) -> Option<String> {
    TOKEN_REGEXES
        .iter()
        .find_map(|regex| regex.captures(html))
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_request_verification_token_name_first() {
        let html = r#"<input name="__RequestVerificationToken" type="hidden" value="TOKEN123" />"#;
        let token = extract_request_verification_token(html);
        assert_eq!(token.as_deref(), Some("TOKEN123"));
    }

    #[test]
    fn extracts_request_verification_token_value_first() {
        let html = r#"<input type="hidden" value="TOKEN456" name="__RequestVerificationToken" />"#;
        let token = extract_request_verification_token(html);
        assert_eq!(token.as_deref(), Some("TOKEN456"));
    }
}
