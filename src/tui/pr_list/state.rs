use crate::models::PullRequest;
use crate::tui::navigation::{PrPane, ViewMode};
use crate::tui::state::tui_attention_score;

const MAX_SYNC_LOG_LINES: usize = 256;

/// State for the PR List screen.
pub struct State {
    /// Which pane is currently focused.
    pub focus: PrPane,
    /// Cursor position in the tracked-authors pane.
    pub tracked_cursor: usize,
    /// Cursor position in the authored-by-me pane.
    pub mine_cursor: usize,
    /// Current view mode (Active or Acknowledged).
    pub view_mode: ViewMode,
    /// Recent sync log lines shown while a sync is running.
    pub sync_logs: Vec<String>,
}

impl State {
    /// Create a new PR List state with default values.
    pub fn new() -> Self {
        Self {
            focus: PrPane::Tracked,
            tracked_cursor: 0,
            mine_cursor: 0,
            view_mode: ViewMode::Active,
            sync_logs: Vec::new(),
        }
    }

    pub fn clear_sync_logs(&mut self) {
        self.sync_logs.clear();
    }

    pub fn push_sync_log(&mut self, line: impl Into<String>) {
        self.sync_logs.push(line.into());
        if self.sync_logs.len() > MAX_SYNC_LOG_LINES {
            let overflow = self.sync_logs.len() - MAX_SYNC_LOG_LINES;
            self.sync_logs.drain(0..overflow);
        }
    }

    fn pane_indices(&self, prs: &[PullRequest], username: &str, pane: PrPane) -> Vec<usize> {
        let mut indices: Vec<usize> = prs
            .iter()
            .enumerate()
            .filter_map(|(index, pr)| {
                let matches_view = match self.view_mode {
                    ViewMode::Active => !pr.is_acknowledged_for_user(username),
                    ViewMode::Acknowledged => pr.is_acknowledged_for_user(username),
                };

                let matches_pane = match pane {
                    PrPane::Tracked => !pr.is_mine(username),
                    PrPane::Mine => pr.is_mine(username),
                };

                if matches_view && matches_pane {
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

    pub fn tracked_indices(&self, prs: &[PullRequest], username: &str) -> Vec<usize> {
        self.pane_indices(prs, username, PrPane::Tracked)
    }

    pub fn mine_indices(&self, prs: &[PullRequest], username: &str) -> Vec<usize> {
        self.pane_indices(prs, username, PrPane::Mine)
    }

    pub fn cursor_for(&self, pane: PrPane) -> usize {
        match pane {
            PrPane::Tracked => self.tracked_cursor,
            PrPane::Mine => self.mine_cursor,
        }
    }

    pub fn cursor_for_mut(&mut self, pane: PrPane) -> &mut usize {
        match pane {
            PrPane::Tracked => &mut self.tracked_cursor,
            PrPane::Mine => &mut self.mine_cursor,
        }
    }

    pub fn selected_index(&self, filtered_indices: &[usize], pane: PrPane) -> Option<usize> {
        filtered_indices.get(self.cursor_for(pane)).copied()
    }

    pub fn focused_pane_indices(&self, prs: &[PullRequest], username: &str) -> Vec<usize> {
        match self.focus {
            PrPane::Tracked => self.tracked_indices(prs, username),
            PrPane::Mine => self.mine_indices(prs, username),
        }
    }

    pub fn selected_index_for_focus(&self, prs: &[PullRequest], username: &str) -> Option<usize> {
        let filtered_indices = self.focused_pane_indices(prs, username);
        self.selected_index(&filtered_indices, self.focus)
    }

    pub fn clamp_cursor(&mut self, pane: PrPane, len: usize) {
        let cursor = self.cursor_for_mut(pane);
        if len == 0 {
            *cursor = 0;
        } else if *cursor >= len {
            *cursor = len - 1;
        }
    }

    pub fn clamp_cursors(&mut self, tracked_len: usize, mine_len: usize) {
        self.clamp_cursor(PrPane::Tracked, tracked_len);
        self.clamp_cursor(PrPane::Mine, mine_len);
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PrPane::Tracked => PrPane::Mine,
            PrPane::Mine => PrPane::Tracked,
        };
    }

    /// Toggle between Active and Acknowledged view modes.
    /// Resets both cursors to 0 when toggling.
    pub fn toggle_view(&mut self) {
        self.view_mode = self.view_mode.toggle();
        self.tracked_cursor = 0;
        self.mine_cursor = 0;
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
            comments: Vec::new(),
        }
    }

    fn pr_with_author(number: i64, author: &str) -> PullRequest {
        let mut pr = test_pr();
        pr.number = number;
        pr.author = author.to_string();
        pr
    }

    fn pr_with_ack(number: i64, author: &str, ack: bool) -> PullRequest {
        let mut pr = pr_with_author(number, author);
        if ack {
            pr.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);
        }
        pr
    }

    #[test]
    fn new_starts_with_tracked_focus() {
        let state = State::new();
        assert!(matches!(state.focus, PrPane::Tracked));
    }

    #[test]
    fn new_starts_with_zero_cursors() {
        let state = State::new();
        assert_eq!(state.tracked_cursor, 0);
        assert_eq!(state.mine_cursor, 0);
    }

    #[test]
    fn new_starts_with_empty_sync_logs() {
        let state = State::new();
        assert!(state.sync_logs.is_empty());
    }

    #[test]
    fn push_sync_log_keeps_recent_entries() {
        let mut state = State::new();

        for index in 0..300 {
            state.push_sync_log(format!("line {index}"));
        }

        assert_eq!(state.sync_logs.len(), 256);
        assert_eq!(state.sync_logs.first(), Some(&"line 44".to_string()));
        assert_eq!(state.sync_logs.last(), Some(&"line 299".to_string()));
    }

    #[test]
    fn toggle_focus_switches_to_mine() {
        let mut state = State::new();
        state.toggle_focus();
        assert!(matches!(state.focus, PrPane::Mine));
    }

    #[test]
    fn toggle_view_resets_both_cursors() {
        let mut state = State::new();
        state.tracked_cursor = 2;
        state.mine_cursor = 3;
        state.toggle_view();
        assert_eq!(state.tracked_cursor, 0);
        assert_eq!(state.mine_cursor, 0);
    }

    #[test]
    fn tracked_indices_exclude_my_prs() {
        let state = State::new();
        let prs = vec![pr_with_author(1, "alice"), pr_with_author(2, "bob")];

        assert_eq!(state.tracked_indices(&prs, "alice"), vec![1]);
    }

    #[test]
    fn mine_indices_include_only_my_prs() {
        let state = State::new();
        let prs = vec![pr_with_author(1, "alice"), pr_with_author(2, "bob")];

        assert_eq!(state.mine_indices(&prs, "alice"), vec![0]);
    }

    #[test]
    fn tracked_indices_filter_acknowledged_by_view_mode() {
        let mut state = State::new();
        state.view_mode = ViewMode::Acknowledged;
        let prs = vec![pr_with_ack(1, "bob", false), pr_with_ack(2, "bob", true)];

        assert_eq!(state.tracked_indices(&prs, "alice"), vec![1]);
    }

    #[test]
    fn mine_indices_keep_my_commit_acknowledged() {
        let mut state = State::new();
        state.view_mode = ViewMode::Acknowledged;
        let mut pr = pr_with_author(1, "alice");
        pr.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);
        pr.last_commit_at = Utc.timestamp_opt(10, 0).unwrap();

        assert_eq!(state.mine_indices(&[pr], "alice"), vec![0]);
    }

    #[test]
    fn clamp_cursors_clamps_each_pane_independently() {
        let mut state = State::new();
        state.tracked_cursor = 10;
        state.mine_cursor = 4;

        state.clamp_cursors(2, 0);

        assert_eq!(state.tracked_cursor, 1);
        assert_eq!(state.mine_cursor, 0);
    }

    #[test]
    fn selected_index_uses_pane_cursor() {
        let mut state = State::new();
        state.mine_cursor = 1;
        let filtered = vec![4, 8, 9];

        assert_eq!(state.selected_index(&filtered, PrPane::Mine), Some(8));
    }

    #[test]
    fn selected_index_for_focus_uses_focused_pane() {
        let mut state = State::new();
        state.focus = PrPane::Mine;
        let prs = vec![pr_with_author(1, "bob"), pr_with_author(2, "alice")];

        assert_eq!(state.selected_index_for_focus(&prs, "alice"), Some(1));
    }

    #[test]
    fn tracked_indices_sort_by_attention_then_updated() {
        let state = State::new();
        let mut pr1 = pr_with_author(1, "bob");
        pr1.requested_reviewers = vec!["alice".to_string()];

        let mut pr2 = pr_with_author(2, "carol");
        pr2.updated_at = Utc.timestamp_opt(100, 0).unwrap();

        let prs = vec![pr2, pr1];

        assert_eq!(state.tracked_indices(&prs, "alice"), vec![1, 0]);
    }
}
