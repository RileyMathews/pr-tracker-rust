use crate::models::PullRequest;
use crate::tui::navigation::ViewMode;
use crate::tui::state::tui_attention_score;

/// State for the PR List screen.
pub struct State {
    /// Cursor position in the filtered list.
    pub cursor: usize,
    /// Current view mode (Active or Acknowledged).
    pub view_mode: ViewMode,
}

impl State {
    /// Create a new PR List state with default values.
    pub fn new() -> Self {
        Self {
            cursor: 0,
            view_mode: ViewMode::Active,
        }
    }

    /// Filter and sort PR indices based on view mode and attention score.
    ///
    /// Returns indices sorted by:
    /// 1. Attention score (descending)
    /// 2. Updated at (descending)
    /// 3. Repository (ascending)
    /// 4. Number (ascending)
    pub fn filtered_indices(&self, prs: &[PullRequest], username: &str) -> Vec<usize> {
        let mut indices: Vec<usize> = prs
            .iter()
            .enumerate()
            .filter_map(|(index, pr)| {
                let include = match self.view_mode {
                    ViewMode::Active => !pr.is_acknowledged(),
                    ViewMode::Acknowledged => pr.is_acknowledged(),
                };

                if include {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        indices.sort_by(|&a, &b| {
            let score_a = tui_attention_score(&prs[a], username);
            let score_b = tui_attention_score(&prs[b], username);
            let pr_a = &prs[a];
            let pr_b = &prs[b];
            score_b
                .cmp(&score_a)
                .then(pr_b.updated_at.cmp(&pr_a.updated_at))
                .then(pr_a.repository.cmp(&pr_b.repository))
                .then(pr_a.number.cmp(&pr_b.number))
        });

        indices
    }

    /// Return the PR index at the current cursor position.
    pub fn selected_index(&self, filtered_indices: &[usize]) -> Option<usize> {
        filtered_indices.get(self.cursor).copied()
    }

    /// Clamp cursor to valid range.
    pub fn ensure_cursor_in_range(&mut self, len: usize) {
        if len == 0 {
            self.cursor = 0;
            return;
        }

        if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    /// Toggle between Active and Acknowledged view modes.
    /// Resets cursor to 0 when toggling.
    pub fn toggle_view(&mut self) {
        self.view_mode = self.view_mode.toggle();
        self.cursor = 0;
    }

    /// Return the label for the current view mode.
    pub fn view_label(&self) -> &'static str {
        self.view_mode.label()
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
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use chrono::{DateTime, TimeZone, Utc};

    fn test_pr() -> PullRequest {
        PullRequest {
            number: 1,
            title: "Test PR".to_string(),
            repository: "owner/repo".to_string(),
            author: "alice".to_string(),
            head_sha: "abc123".to_string(),
            draft: false,
            created_at: DateTime::UNIX_EPOCH,
            updated_at: DateTime::UNIX_EPOCH,
            ci_status: CiStatus::Pending,
            last_comment_at: DateTime::UNIX_EPOCH,
            last_commit_at: DateTime::UNIX_EPOCH,
            last_ci_status_update_at: DateTime::UNIX_EPOCH,
            approval_status: ApprovalStatus::None,
            last_review_status_update_at: DateTime::UNIX_EPOCH,
            last_acknowledged_at: None,
            requested_reviewers: Vec::new(),
            user_has_reviewed: false,
            comments: vec![],
        }
    }

    fn pr_with_ack(number: i64, ack: bool) -> PullRequest {
        let mut pr = test_pr();
        pr.number = number;
        if ack {
            pr.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);
        }
        pr
    }

    // ── State::new tests ──────────────────────────────────────────────

    #[test]
    fn new_starts_at_cursor_zero() {
        let state = State::new();
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn new_starts_with_active_view() {
        let state = State::new();
        assert!(matches!(state.view_mode, ViewMode::Active));
    }

    // ── State::toggle_view tests ───────────────────────────────────

    #[test]
    fn toggle_view_switches_to_acknowledged() {
        let mut state = State::new();
        state.toggle_view();
        assert!(matches!(state.view_mode, ViewMode::Acknowledged));
    }

    #[test]
    fn toggle_view_resets_cursor_to_zero() {
        let mut state = State::new();
        state.cursor = 5;
        state.toggle_view();
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn toggle_view_twice_returns_to_active() {
        let mut state = State::new();
        state.toggle_view();
        state.toggle_view();
        assert!(matches!(state.view_mode, ViewMode::Active));
    }

    // ── State::ensure_cursor_in_range tests ─────────────────────────

    #[test]
    fn ensure_cursor_in_range_empty_list_sets_zero() {
        let mut state = State::new();
        state.cursor = 5;
        state.ensure_cursor_in_range(0);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn ensure_cursor_in_range_clamps_when_beyond() {
        let mut state = State::new();
        state.cursor = 10;
        state.ensure_cursor_in_range(5); // only 5 items
        assert_eq!(state.cursor, 4); // clamped to len-1
    }

    #[test]
    fn ensure_cursor_in_range_unchanged_when_valid() {
        let mut state = State::new();
        state.cursor = 3;
        state.ensure_cursor_in_range(10);
        assert_eq!(state.cursor, 3);
    }

    #[test]
    fn ensure_cursor_in_range_handles_cursor_at_boundary() {
        let mut state = State::new();
        state.cursor = 4;
        state.ensure_cursor_in_range(5);
        assert_eq!(state.cursor, 4); // valid, should stay
    }

    // ── State::filtered_indices tests ────────────────────────────────

    #[test]
    fn filtered_indices_active_view_filters_non_acknowledged() {
        let state = State::new();
        let prs = vec![
            pr_with_ack(1, false),
            pr_with_ack(2, true),
            pr_with_ack(3, false),
        ];

        let indices = state.filtered_indices(&prs, "bob");

        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn filtered_indices_acknowledged_view_filters_acknowledged() {
        let mut state = State::new();
        state.view_mode = ViewMode::Acknowledged;
        let prs = vec![
            pr_with_ack(1, false),
            pr_with_ack(2, true),
            pr_with_ack(3, false),
        ];

        let indices = state.filtered_indices(&prs, "bob");

        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn filtered_indices_empty_list_returns_empty() {
        let state = State::new();
        let prs: Vec<PullRequest> = vec![];

        let indices = state.filtered_indices(&prs, "bob");

        assert!(indices.is_empty());
    }

    #[test]
    fn filtered_indices_sorts_by_attention_score() {
        let state = State::new();
        let mut pr1 = test_pr();
        pr1.number = 1;
        pr1.requested_reviewers = vec!["bob".to_string()];

        let mut pr2 = test_pr();
        pr2.number = 2;

        let prs = vec![pr2, pr1];

        let indices = state.filtered_indices(&prs, "bob");

        // PR 1 has higher score (involved)
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn filtered_indices_sorts_by_updated_at_when_scores_equal() {
        let state = State::new();
        let mut pr1 = test_pr();
        pr1.number = 1;
        pr1.updated_at = Utc.timestamp_opt(100, 0).unwrap();

        let mut pr2 = test_pr();
        pr2.number = 2;
        pr2.updated_at = Utc.timestamp_opt(200, 0).unwrap();

        let prs = vec![pr1, pr2];

        let indices = state.filtered_indices(&prs, "bob");

        // Later updated_at comes first
        assert_eq!(indices, vec![1, 0]);
    }

    // ── State::selected_index tests ─────────────────────────────────

    #[test]
    fn selected_index_returns_correct_index() {
        let state = State::new();
        let filtered = vec![2, 0, 1];

        assert_eq!(state.selected_index(&filtered), Some(2));
    }

    #[test]
    fn selected_index_with_cursor_returns_correct() {
        let mut state = State::new();
        state.cursor = 1;
        let filtered = vec![2, 0, 1];

        assert_eq!(state.selected_index(&filtered), Some(0));
    }

    #[test]
    fn selected_index_returns_none_when_out_of_range() {
        let mut state = State::new();
        state.cursor = 10;
        let filtered = vec![0, 1];

        assert_eq!(state.selected_index(&filtered), None);
    }

    #[test]
    fn selected_index_returns_none_with_empty_list() {
        let state = State::new();
        let filtered: Vec<usize> = vec![];

        assert_eq!(state.selected_index(&filtered), None);
    }

    // ── view_label tests ───────────────────────────────────────────

    #[test]
    fn view_label_active() {
        let state = State::new();
        assert_eq!(state.view_label(), "active");
    }

    #[test]
    fn view_label_acknowledged() {
        let mut state = State::new();
        state.view_mode = ViewMode::Acknowledged;
        assert_eq!(state.view_label(), "acknowledged");
    }
}
