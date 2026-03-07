use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::tui::navigation::AuthorsPane;

/// State for the Authors from Teams screen.
pub struct State {
    /// List of tracked authors.
    pub tracked: Vec<String>,
    /// List of untracked authors.
    pub untracked: Vec<String>,
    /// Which pane is currently focused (Tracked or Untracked).
    pub focus: AuthorsPane,
    /// Cursor position in the tracked list.
    pub tracked_cursor: usize,
    /// Cursor position in the untracked list.
    pub untracked_cursor: usize,
    /// Current search filter query.
    pub search_query: String,
    /// Whether data is currently loading.
    pub loading: bool,
    /// Error message if loading failed.
    pub error: Option<String>,
}

impl State {
    /// Create a new Authors screen state with default values.
    /// Starts with loading=true, focus on Untracked, empty lists.
    pub fn new() -> Self {
        Self {
            tracked: Vec::new(),
            untracked: Vec::new(),
            focus: AuthorsPane::Untracked,
            tracked_cursor: 0,
            untracked_cursor: 0,
            search_query: String::new(),
            loading: true,
            error: None,
        }
    }

    /// Ensure cursors don't exceed list bounds.
    pub fn clamp_cursors(&mut self) {
        if self.tracked.is_empty() {
            self.tracked_cursor = 0;
        } else if self.tracked_cursor >= self.tracked.len() {
            self.tracked_cursor = self.tracked.len() - 1;
        }
        if self.untracked.is_empty() {
            self.untracked_cursor = 0;
        } else if self.untracked_cursor >= self.untracked.len() {
            self.untracked_cursor = self.untracked.len() - 1;
        }
    }

    /// Filter and score a list by search_query using fuzzy matching.
    ///
    /// Returns `(original_index, &login)` pairs, filtered and scored by search_query.
    /// When query is empty, returns all items in order sorted by fuzzy match score.
    pub fn filtered_list<'a>(&self, list: &'a [String]) -> Vec<(usize, &'a String)> {
        // Returns (original_index, &login) pairs, filtered and scored by search_query.
        // When query is empty, returns all items in order.
        if self.search_query.is_empty() {
            return list.iter().enumerate().collect();
        }
        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, usize, &String)> = list
            .iter()
            .enumerate()
            .filter_map(|(i, login)| {
                matcher
                    .fuzzy_match(login, &self.search_query)
                    .map(|score| (score, i, login))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, i, login)| (i, login)).collect()
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── State::new tests ────────────────────────────────────────────

    #[test]
    fn new_starts_with_loading_true() {
        let state = State::new();
        assert!(state.loading);
    }

    #[test]
    fn new_starts_with_focus_on_untracked() {
        let state = State::new();
        assert!(matches!(state.focus, AuthorsPane::Untracked));
    }

    #[test]
    fn new_starts_with_empty_lists() {
        let state = State::new();
        assert!(state.tracked.is_empty());
        assert!(state.untracked.is_empty());
    }

    #[test]
    fn new_starts_with_zero_cursors() {
        let state = State::new();
        assert_eq!(state.tracked_cursor, 0);
        assert_eq!(state.untracked_cursor, 0);
    }

    #[test]
    fn new_starts_with_empty_search_query() {
        let state = State::new();
        assert_eq!(state.search_query, "");
    }

    #[test]
    fn new_starts_with_no_error() {
        let state = State::new();
        assert!(state.error.is_none());
    }

    // ── State::clamp_cursors tests ─────────────────────────────────

    #[test]
    fn clamp_cursors_empty_tracked_sets_zero() {
        let mut state = State::new();
        state.tracked_cursor = 5;
        state.clamp_cursors();
        assert_eq!(state.tracked_cursor, 0);
    }

    #[test]
    fn clamp_cursors_empty_untracked_sets_zero() {
        let mut state = State::new();
        state.untracked_cursor = 5;
        state.clamp_cursors();
        assert_eq!(state.untracked_cursor, 0);
    }

    #[test]
    fn clamp_cursors_tracked_clamps_when_beyond() {
        let mut state = State::new();
        state.tracked = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        state.tracked_cursor = 10;
        state.clamp_cursors();
        assert_eq!(state.tracked_cursor, 2); // len - 1
    }

    #[test]
    fn clamp_cursors_untracked_clamps_when_beyond() {
        let mut state = State::new();
        state.untracked = vec!["x".to_string(), "y".to_string()];
        state.untracked_cursor = 5;
        state.clamp_cursors();
        assert_eq!(state.untracked_cursor, 1); // len - 1
    }

    #[test]
    fn clamp_cursors_valid_cursor_unchanged() {
        let mut state = State::new();
        state.tracked = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        state.tracked_cursor = 1;
        state.clamp_cursors();
        assert_eq!(state.tracked_cursor, 1);
    }

    #[test]
    fn clamp_cursors_handles_both_lists() {
        let mut state = State::new();
        state.tracked = vec!["a".to_string()];
        state.untracked = vec!["x".to_string(), "y".to_string()];
        state.tracked_cursor = 5;
        state.untracked_cursor = 10;

        state.clamp_cursors();

        assert_eq!(state.tracked_cursor, 0); // 1 item, so max index is 0
        assert_eq!(state.untracked_cursor, 1); // 2 items, so max index is 1
    }

    // ── State::filtered_list tests ──────────────────────────────────

    #[test]
    fn filtered_list_empty_query_returns_all() {
        let state = State::new();
        let list = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];

        let result = state.filtered_list(&list);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, 0);
        assert_eq!(result[0].1, "alice");
        assert_eq!(result[1].0, 1);
        assert_eq!(result[1].1, "bob");
        assert_eq!(result[2].0, 2);
        assert_eq!(result[2].1, "charlie");
    }

    #[test]
    fn filtered_list_empty_query_preserves_order() {
        let state = State::new();
        let list = vec!["zulu".to_string(), "alpha".to_string(), "mike".to_string()];

        let result = state.filtered_list(&list);

        assert_eq!(result[0].1, "zulu");
        assert_eq!(result[1].1, "alpha");
        assert_eq!(result[2].1, "mike");
    }

    #[test]
    fn filtered_list_case_insensitive() {
        let mut state = State::new();
        state.search_query = "ALICE".to_string();
        let list = vec![
            "alice".to_string(),
            "Alice".to_string(),
            "ALICE".to_string(),
        ];

        let result = state.filtered_list(&list);

        // Should find at least one match (fuzzy matcher is case-insensitive)
        assert!(!result.is_empty());
        // All items are the same word, just different case
        assert!(result
            .iter()
            .any(|(_, name)| name.to_lowercase() == "alice"));
    }

    #[test]
    fn filtered_list_no_matches_returns_empty() {
        let mut state = State::new();
        state.search_query = "xyz".to_string();
        let list = vec!["alice".to_string(), "bob".to_string()];

        let result = state.filtered_list(&list);

        assert!(result.is_empty());
    }

    #[test]
    fn filtered_list_returns_original_indices() {
        let mut state = State::new();
        state.search_query = "bob".to_string();
        let list = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];

        let result = state.filtered_list(&list);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 1); // original index
        assert_eq!(result[0].1, "bob");
    }

    #[test]
    fn filtered_list_sorts_by_match_score() {
        let mut state = State::new();
        state.search_query = "test".to_string();
        let list = vec![
            "this is a test".to_string(),
            "test".to_string(),
            "testing stuff".to_string(),
        ];

        let result = state.filtered_list(&list);

        // Exact match "test" should be first (highest score)
        assert_eq!(result[0].1, "test");
    }

    #[test]
    fn filtered_list_empty_list_returns_empty() {
        let state = State::new();
        let list: Vec<String> = vec![];

        let result = state.filtered_list(&list);

        assert!(result.is_empty());
    }

    #[test]
    fn filtered_list_empty_query_with_empty_list() {
        let state = State::new();
        let list: Vec<String> = vec![];

        let result = state.filtered_list(&list);

        assert!(result.is_empty());
    }
}
