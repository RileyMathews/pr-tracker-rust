use crate::models::PullRequest;
use crate::scoring;

/// Shared state across all screens — PR data and username.
pub struct SharedState {
    pub prs: Vec<PullRequest>,
    pub username: String,
}

impl SharedState {
    pub fn new(prs: Vec<PullRequest>, username: String) -> Self {
        Self { prs, username }
    }

    pub fn filtered_indices(&self, view_mode: super::navigation::ViewMode) -> Vec<usize> {
        let mut indices: Vec<usize> = self
            .prs
            .iter()
            .enumerate()
            .filter_map(|(index, pr)| {
                let include = match view_mode {
                    super::navigation::ViewMode::Active => !pr.is_acknowledged(),
                    super::navigation::ViewMode::Acknowledged => pr.is_acknowledged(),
                };

                if include {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        indices.sort_by(|&a, &b| {
            let score_a = tui_attention_score(&self.prs[a], &self.username);
            let score_b = tui_attention_score(&self.prs[b], &self.username);
            let pr_a = &self.prs[a];
            let pr_b = &self.prs[b];
            score_b
                .cmp(&score_a)
                .then(pr_b.updated_at.cmp(&pr_a.updated_at))
                .then(pr_a.repository.cmp(&pr_b.repository))
                .then(pr_a.number.cmp(&pr_b.number))
        });

        indices
    }

    pub fn selected_index(&self, filtered_indices: &[usize], cursor: usize) -> Option<usize> {
        filtered_indices.get(cursor).copied()
    }
}

/// Pure utility function: calculate attention score for TUI sorting.
/// Has zero terminal dependencies and is fully testable.
pub fn tui_attention_score(pr: &PullRequest, username: &str) -> i64 {
    let mut score = scoring::importance_score(pr, username);
    if pr.user_is_involved(username) {
        score += 100;
    }
    score
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
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use crate::tui::navigation::ViewMode;
    use chrono::{DateTime, Utc};

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
            comments: Vec::new(),
        }
    }

    // ── tui_attention_score tests ────────────────────────────────────

    #[test]
    fn tui_attention_score_includes_importance_score() {
        let pr = test_pr();
        let base_score = crate::scoring::importance_score(&pr, "bob");

        let tui_score = tui_attention_score(&pr, "bob");

        // Since user is not involved, tui score should equal base score
        assert_eq!(tui_score, base_score);
    }

    #[test]
    fn tui_attention_score_adds_bonus_for_involved_user() {
        let mut pr = test_pr();
        pr.requested_reviewers = vec!["bob".to_string()];

        let base_score = crate::scoring::importance_score(&pr, "bob");
        let tui_score = tui_attention_score(&pr, "bob");

        // Should add +100 bonus for involved user
        assert_eq!(tui_score, base_score + 100);
    }

    #[test]
    fn tui_attention_score_adds_bonus_for_author() {
        let pr = test_pr();

        let base_score = crate::scoring::importance_score(&pr, "alice");
        let tui_score = tui_attention_score(&pr, "alice");

        // Author is involved, so should add +100 bonus
        assert_eq!(tui_score, base_score + 100);
    }

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

    // ── SharedState::filtered_indices tests ─────────────────────────

    fn pr_with_ack(number: i64, ack: bool) -> PullRequest {
        let mut pr = test_pr();
        pr.number = number;
        if ack {
            pr.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);
        }
        pr
    }

    #[test]
    fn filtered_indices_active_view_excludes_acknowledged() {
        let prs = vec![
            pr_with_ack(1, false), // not acknowledged
            pr_with_ack(2, true),  // acknowledged
            pr_with_ack(3, false), // not acknowledged
        ];
        let state = SharedState::new(prs, "alice".to_string());

        let indices = state.filtered_indices(ViewMode::Active);

        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn filtered_indices_acknowledged_view_includes_only_acknowledged() {
        let prs = vec![
            pr_with_ack(1, false), // not acknowledged
            pr_with_ack(2, true),  // acknowledged
            pr_with_ack(3, false), // not acknowledged
        ];
        let state = SharedState::new(prs, "alice".to_string());

        let indices = state.filtered_indices(ViewMode::Acknowledged);

        assert_eq!(indices, vec![1]);
    }

    #[test]
    fn filtered_indices_empty_list_returns_empty() {
        let state = SharedState::new(vec![], "alice".to_string());

        let indices = state.filtered_indices(ViewMode::Active);

        assert!(indices.is_empty());
    }

    #[test]
    fn filtered_indices_sorts_by_attention_score_desc() {
        let mut pr1 = test_pr();
        pr1.number = 1;
        pr1.requested_reviewers = vec!["bob".to_string()]; // higher score for bob

        let mut pr2 = test_pr();
        pr2.number = 2;
        // not involved, lower score

        let prs = vec![pr2.clone(), pr1.clone()];
        let state = SharedState::new(prs, "bob".to_string());

        let indices = state.filtered_indices(ViewMode::Active);

        // PR 1 (involved) should come before PR 2
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn filtered_indices_sorts_by_updated_at_desc_when_scores_equal() {
        use chrono::TimeZone;

        let mut pr1 = test_pr();
        pr1.number = 1;
        pr1.updated_at = Utc.timestamp_opt(100, 0).unwrap();

        let mut pr2 = test_pr();
        pr2.number = 2;
        pr2.updated_at = Utc.timestamp_opt(200, 0).unwrap(); // later

        let prs = vec![pr1, pr2];
        let state = SharedState::new(prs, "bob".to_string());

        let indices = state.filtered_indices(ViewMode::Active);

        // PR 2 (later updated_at) should come first
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn filtered_indices_sorts_by_repo_asc_when_scores_and_updated_equal() {
        let mut pr1 = test_pr();
        pr1.number = 1;
        pr1.repository = "owner/repo-b".to_string();

        let mut pr2 = test_pr();
        pr2.number = 2;
        pr2.repository = "owner/repo-a".to_string(); // alphabetically first

        let prs = vec![pr1, pr2];
        let state = SharedState::new(prs, "bob".to_string());

        let indices = state.filtered_indices(ViewMode::Active);

        // PR 2 (repo-a) should come before PR 1 (repo-b)
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn filtered_indices_sorts_by_number_asc_when_all_else_equal() {
        let mut pr1 = test_pr();
        pr1.number = 100;

        let mut pr2 = test_pr();
        pr2.number = 50; // lower number

        let prs = vec![pr1, pr2];
        let state = SharedState::new(prs, "bob".to_string());

        let indices = state.filtered_indices(ViewMode::Active);

        // PR 2 (number 50) should come before PR 1 (number 100)
        assert_eq!(indices, vec![1, 0]);
    }

    // ── SharedState::selected_index tests ───────────────────────────

    #[test]
    fn selected_index_returns_correct_pr_index() {
        let prs = vec![test_pr(), test_pr(), test_pr()];
        let state = SharedState::new(prs, "alice".to_string());
        let filtered = vec![2, 0, 1]; // filtered order

        assert_eq!(state.selected_index(&filtered, 0), Some(2));
        assert_eq!(state.selected_index(&filtered, 1), Some(0));
        assert_eq!(state.selected_index(&filtered, 2), Some(1));
    }

    #[test]
    fn selected_index_returns_none_when_cursor_out_of_range() {
        let prs = vec![test_pr()];
        let state = SharedState::new(prs, "alice".to_string());
        let filtered = vec![0];

        assert_eq!(state.selected_index(&filtered, 5), None);
    }

    #[test]
    fn selected_index_returns_none_with_empty_filtered_list() {
        let prs = vec![];
        let state = SharedState::new(prs, "alice".to_string());
        let filtered: Vec<usize> = vec![];

        assert_eq!(state.selected_index(&filtered, 0), None);
    }
}
