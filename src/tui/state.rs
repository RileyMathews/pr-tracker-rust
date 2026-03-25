use crate::pr_repository::PrDashboard;

/// Shared state across all screens — PR data and username.
pub struct SharedState {
    pub dashboard: PrDashboard,
    pub username: String,
    pub error: Option<String>,
}

impl SharedState {
    pub fn new(dashboard: PrDashboard, username: String) -> Self {
        Self {
            dashboard,
            username,
            error: None,
        }
    }
}

/// Pure utility function: convert string to title case.
/// Has zero terminal dependencies and is fully testable.
pub fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
}

/// Pure utility function: truncate string to max_chars, adding "..." if truncated.
/// Has zero terminal dependencies and is fully testable.
pub fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── title_case tests ───────────────────────────────────────────

    #[test]
    fn title_case_empty_string_returns_empty() {
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn title_case_single_char_returns_uppercase() {
        assert_eq!(title_case("a"), "A");
        assert_eq!(title_case("z"), "Z");
    }

    #[test]
    fn title_case_multiple_chars_uppercases_first() {
        assert_eq!(title_case("hello"), "Hello");
        assert_eq!(title_case("HELLO"), "HELLO");
        assert_eq!(title_case("hELLO"), "HELLO");
    }

    #[test]
    fn title_case_preserves_rest_unchanged() {
        assert_eq!(title_case("test"), "Test");
        assert_eq!(title_case("TEST"), "TEST");
        assert_eq!(title_case("tEsT"), "TEsT");
    }

    // ── truncate tests ─────────────────────────────────────────────

    #[test]
    fn truncate_shorter_than_max_returns_unchanged() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exactly_max_returns_unchanged() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_longer_adds_ellipsis() {
        assert_eq!(truncate("hello world", 5), "hello...");
    }

    #[test]
    fn truncate_zero_max_returns_ellipsis() {
        // Taking 0 chars, then finding there's more, adds "..." to empty string
        assert_eq!(truncate("hello", 0), "...");
    }

    #[test]
    fn truncate_unicode_respects_char_count() {
        // Unicode chars should be counted correctly
        assert_eq!(truncate("héllo", 3), "hél...");
    }
}
