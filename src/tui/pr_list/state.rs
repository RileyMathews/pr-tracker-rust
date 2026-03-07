use crossterm::event::KeyCode;

use crate::models::PullRequest;
use crate::tui::navigation::{Screen, ViewMode};
use crate::tui::state::{tui_attention_score, SharedState};

/// State for the PR List screen.
pub struct State {
    /// Cursor position in the filtered list.
    pub cursor: usize,
    /// Current view mode (Active or Acknowledged).
    pub view_mode: ViewMode,
}

/// Pure intent emitted by the PR List reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    SwitchScreen(Screen),
    OpenSelectedPr { pr_index: usize },
    AcknowledgeSelectedPr { pr_index: usize },
    TriggerSync,
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

    /// Pure reducer for PR List key handling.
    pub fn reduce_key(
        &mut self,
        key_code: KeyCode,
        shared: &SharedState,
        has_active_job: bool,
    ) -> Action {
        let filtered_indices = self.filtered_indices(&shared.prs, &shared.username);
        self.ensure_cursor_in_range(filtered_indices.len());

        match key_code {
            KeyCode::Char('q') => Action::Quit,

            KeyCode::Up | KeyCode::Char('k') => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                Action::None
            }

            KeyCode::Down | KeyCode::Char('j') => {
                if self.cursor + 1 < filtered_indices.len() {
                    self.cursor += 1;
                }
                Action::None
            }

            KeyCode::Enter | KeyCode::Char(' ') => self
                .selected_index(&filtered_indices)
                .map_or(Action::None, |pr_index| Action::OpenSelectedPr { pr_index }),

            KeyCode::Char('a') => self
                .selected_index(&filtered_indices)
                .map_or(Action::None, |pr_index| Action::AcknowledgeSelectedPr {
                    pr_index,
                }),

            KeyCode::Char('v') => {
                self.toggle_view();
                let updated_filtered_indices = self.filtered_indices(&shared.prs, &shared.username);
                self.ensure_cursor_in_range(updated_filtered_indices.len());
                Action::None
            }

            KeyCode::Char('s') => {
                if has_active_job {
                    Action::None
                } else {
                    Action::TriggerSync
                }
            }

            KeyCode::Char('t') => {
                if has_active_job {
                    Action::None
                } else {
                    Action::SwitchScreen(Screen::AuthorsFromTeams)
                }
            }

            _ => Action::None,
        }
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
    use chrono::DateTime;

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

    fn test_shared(prs: Vec<PullRequest>) -> SharedState {
        SharedState::new(prs, "alice".to_string())
    }

    fn pr_with_ack(number: i64, ack: bool) -> PullRequest {
        let mut pr = test_pr();
        pr.number = number;
        if ack {
            pr.last_acknowledged_at = Some(DateTime::UNIX_EPOCH);
        }
        pr
    }

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
        state.ensure_cursor_in_range(5);
        assert_eq!(state.cursor, 4);
    }

    #[test]
    fn reduce_key_up_stops_at_zero() {
        let mut state = State::new();
        let shared = test_shared(vec![test_pr()]);

        let action = state.reduce_key(KeyCode::Up, &shared, false);

        assert_eq!(action, Action::None);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn reduce_key_down_stops_at_last_item() {
        let mut state = State::new();
        state.cursor = 1;
        let mut pr2 = test_pr();
        pr2.number = 2;
        let shared = test_shared(vec![test_pr(), pr2]);

        let action = state.reduce_key(KeyCode::Down, &shared, false);

        assert_eq!(action, Action::None);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn reduce_key_view_toggle_resets_cursor() {
        let mut state = State::new();
        state.cursor = 3;
        let shared = test_shared(vec![pr_with_ack(1, false), pr_with_ack(2, true)]);

        let action = state.reduce_key(KeyCode::Char('v'), &shared, false);

        assert_eq!(action, Action::None);
        assert!(matches!(state.view_mode, ViewMode::Acknowledged));
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn reduce_key_enter_returns_open_selected_action() {
        let mut state = State::new();
        let shared = test_shared(vec![test_pr()]);

        let action = state.reduce_key(KeyCode::Enter, &shared, false);

        assert_eq!(action, Action::OpenSelectedPr { pr_index: 0 });
    }

    #[test]
    fn reduce_key_acknowledge_returns_ack_action() {
        let mut state = State::new();
        let shared = test_shared(vec![test_pr()]);

        let action = state.reduce_key(KeyCode::Char('a'), &shared, false);

        assert_eq!(action, Action::AcknowledgeSelectedPr { pr_index: 0 });
    }

    #[test]
    fn reduce_key_sync_ignored_when_job_active() {
        let mut state = State::new();
        let shared = test_shared(vec![test_pr()]);

        let action = state.reduce_key(KeyCode::Char('s'), &shared, true);

        assert_eq!(action, Action::None);
    }

    #[test]
    fn reduce_key_sync_triggers_when_no_job() {
        let mut state = State::new();
        let shared = test_shared(vec![test_pr()]);

        let action = state.reduce_key(KeyCode::Char('s'), &shared, false);

        assert_eq!(action, Action::TriggerSync);
    }

    #[test]
    fn reduce_key_switch_screen_when_no_job() {
        let mut state = State::new();
        let shared = test_shared(vec![test_pr()]);

        let action = state.reduce_key(KeyCode::Char('t'), &shared, false);

        assert_eq!(action, Action::SwitchScreen(Screen::AuthorsFromTeams));
    }
}
