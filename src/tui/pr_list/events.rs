use crossterm::event::KeyCode;

use chrono::{DateTime, Utc};

use crate::db::DatabaseRepository;
use crate::models::PullRequest;
use crate::tui::navigation::Screen;
use crate::tui::pr_list::State;
use crate::tui::state::SharedState;
use crate::tui::tasks::{spawn_full_sync, BackgroundJob, BackgroundMessage};

use tokio::sync::mpsc;

/// Result of handling a key event.
pub enum EventResult {
    /// Continue running the TUI (no action needed).
    Continue,
    /// Quit the TUI application.
    Quit,
    /// Switch to a different screen.
    SwitchScreen(Screen),
}

/// A pure description of what side effect to perform.
/// Returned by the pure `resolve_event` function; executed by the impure shell.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// No side effect needed.
    None,
    /// Quit the TUI.
    Quit,
    /// Open a URL in the default browser.
    OpenUrl(String),
    /// Acknowledge a PR: update the PR at `pr_index` in `shared.prs` with the given timestamp and persist.
    AcknowledgePr {
        pr_index: usize,
        now: DateTime<Utc>,
    },
    /// Toggle between Active and Acknowledged views.
    ToggleView,
    /// Start a full sync job.
    StartSync,
    /// Switch to the Authors screen.
    SwitchToAuthors,
}

/// Pure function: given the current state and a key press, decide what action to take.
///
/// This function only reads state and returns a description of the desired effect.
/// It does NOT perform any I/O, database writes, or state mutation.
pub fn resolve_event(
    key_code: KeyCode,
    state: &State,
    shared: &SharedState,
    has_active_job: bool,
) -> Action {
    let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);

    match key_code {
        KeyCode::Char('q') => Action::Quit,

        KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
            // Navigation is handled purely by apply_action on the state
            Action::None
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            let mut cursor = state.cursor;
            if !filtered_indices.is_empty() && cursor >= filtered_indices.len() {
                cursor = filtered_indices.len() - 1;
            }
            match filtered_indices.get(cursor) {
                Some(&pr_index) => Action::OpenUrl(shared.prs[pr_index].url()),
                None => Action::None,
            }
        }

        KeyCode::Char('a') => {
            let mut cursor = state.cursor;
            if !filtered_indices.is_empty() && cursor >= filtered_indices.len() {
                cursor = filtered_indices.len() - 1;
            }
            match filtered_indices.get(cursor) {
                Some(&pr_index) => Action::AcknowledgePr {
                    pr_index,
                    now: Utc::now(),
                },
                None => Action::None,
            }
        }

        KeyCode::Char('v') => Action::ToggleView,

        KeyCode::Char('s') => {
            if has_active_job {
                Action::None
            } else {
                Action::StartSync
            }
        }

        KeyCode::Char('t') => {
            if has_active_job {
                Action::None
            } else {
                Action::SwitchToAuthors
            }
        }

        _ => Action::None,
    }
}

/// Pure function: apply navigation-related key events to state.
///
/// Handles cursor movement and view toggling without any I/O.
pub fn apply_navigation(
    key_code: KeyCode,
    state: &mut State,
    shared: &SharedState,
) {
    let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
    state.ensure_cursor_in_range(filtered_indices.len());

    match key_code {
        KeyCode::Up | KeyCode::Char('k') => {
            if state.cursor > 0 {
                state.cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.cursor + 1 < filtered_indices.len() {
                state.cursor += 1;
            }
        }
        _ => {}
    }
}

/// Pure function: apply an acknowledge action to the shared state.
///
/// Returns the updated PR for persistence by the imperative shell.
pub fn apply_acknowledge(
    shared: &mut SharedState,
    state: &mut State,
    pr_index: usize,
    now: DateTime<Utc>,
) -> Option<PullRequest> {
    if pr_index >= shared.prs.len() {
        return None;
    }
    let mut pr = shared.prs[pr_index].clone();
    pr.last_acknowledged_at = Some(now);
    shared.prs[pr_index] = pr.clone();

    let updated_filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
    state.ensure_cursor_in_range(updated_filtered_indices.len());

    Some(pr)
}

/// Imperative shell: execute an action, performing all necessary I/O.
///
/// This is the only function in this module that performs side effects.
pub async fn execute_action(
    action: Action,
    state: &mut State,
    shared: &mut SharedState,
    repo: &DatabaseRepository,
    tx: &mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<EventResult> {
    match action {
        Action::None => Ok(EventResult::Continue),
        Action::Quit => Ok(EventResult::Quit),

        Action::OpenUrl(url) => {
            let _ = open::that(url);
            Ok(EventResult::Continue)
        }

        Action::AcknowledgePr { pr_index, now } => {
            if let Some(pr) = apply_acknowledge(shared, state, pr_index, now) {
                repo.save_pr(&pr).await?;
            }
            Ok(EventResult::Continue)
        }

        Action::ToggleView => {
            state.toggle_view();
            let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
            state.ensure_cursor_in_range(filtered_indices.len());
            Ok(EventResult::Continue)
        }

        Action::StartSync => {
            spawn_full_sync(repo.clone(), tx.clone());
            Ok(EventResult::Continue)
        }

        Action::SwitchToAuthors => Ok(EventResult::SwitchScreen(Screen::AuthorsFromTeams)),
    }
}

/// Top-level handler: resolves the event purely, then executes the resulting action.
///
/// This is the entry point called by the TUI event loop.
pub async fn handle_event(
    key_code: KeyCode,
    state: &mut State,
    shared: &mut SharedState,
    active_job: &Option<BackgroundJob>,
    repo: &DatabaseRepository,
    tx: &mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<EventResult> {
    // Pure: apply navigation state changes
    apply_navigation(key_code, state, shared);

    // Pure: decide what action to take
    let action = resolve_event(key_code, state, shared, active_job.is_some());

    // Impure: execute the action
    execute_action(action, state, shared, repo, tx).await
}

/// Helper function to get the currently selected PR.
/// Used when acknowledging or opening PRs.
pub fn get_selected_pr<'a>(
    state: &State,
    shared: &'a SharedState,
) -> Option<&'a PullRequest> {
    let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
    state
        .selected_index(&filtered_indices)
        .and_then(|index| shared.prs.get(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use chrono::{DateTime, TimeZone};

    fn test_pr(number: i64) -> PullRequest {
        PullRequest {
            number,
            title: format!("PR #{number}"),
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

    // ── resolve_event tests ──────────────────────────────────────────

    #[test]
    fn resolve_quit() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('q'), &state, &shared, false), Action::Quit);
    }

    #[test]
    fn resolve_navigation_returns_none() {
        let state = State::new();
        let shared = SharedState::new(vec![test_pr(1)], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Up, &state, &shared, false), Action::None);
        assert_eq!(resolve_event(KeyCode::Down, &state, &shared, false), Action::None);
        assert_eq!(resolve_event(KeyCode::Char('k'), &state, &shared, false), Action::None);
        assert_eq!(resolve_event(KeyCode::Char('j'), &state, &shared, false), Action::None);
    }

    #[test]
    fn resolve_open_url_returns_url_for_selected_pr() {
        let state = State::new();
        let shared = SharedState::new(vec![test_pr(42)], "bob".to_string());
        let action = resolve_event(KeyCode::Enter, &state, &shared, false);
        assert_eq!(action, Action::OpenUrl("https://github.com/owner/repo/pull/42".to_string()));
    }

    #[test]
    fn resolve_open_url_on_empty_list() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Enter, &state, &shared, false), Action::None);
    }

    #[test]
    fn resolve_acknowledge_returns_pr_index() {
        let state = State::new();
        let shared = SharedState::new(vec![test_pr(1)], "bob".to_string());
        let action = resolve_event(KeyCode::Char('a'), &state, &shared, false);
        match action {
            Action::AcknowledgePr { pr_index, .. } => assert_eq!(pr_index, 0),
            other => panic!("expected AcknowledgePr, got {:?}", other),
        }
    }

    #[test]
    fn resolve_acknowledge_on_empty_list() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('a'), &state, &shared, false), Action::None);
    }

    #[test]
    fn resolve_toggle_view() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('v'), &state, &shared, false), Action::ToggleView);
    }

    #[test]
    fn resolve_sync_when_no_active_job() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('s'), &state, &shared, false), Action::StartSync);
    }

    #[test]
    fn resolve_sync_when_job_active() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('s'), &state, &shared, true), Action::None);
    }

    #[test]
    fn resolve_switch_to_authors_when_no_job() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('t'), &state, &shared, false), Action::SwitchToAuthors);
    }

    #[test]
    fn resolve_switch_to_authors_blocked_by_active_job() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('t'), &state, &shared, true), Action::None);
    }

    #[test]
    fn resolve_unknown_key_returns_none() {
        let state = State::new();
        let shared = SharedState::new(vec![], "bob".to_string());
        assert_eq!(resolve_event(KeyCode::Char('x'), &state, &shared, false), Action::None);
    }

    // ── apply_navigation tests ──────────────────────────────────────

    #[test]
    fn navigation_up_decrements_cursor() {
        let mut state = State::new();
        state.cursor = 1;
        let shared = SharedState::new(vec![test_pr(1), test_pr(2)], "bob".to_string());

        apply_navigation(KeyCode::Up, &mut state, &shared);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn navigation_up_at_zero_stays() {
        let mut state = State::new();
        let shared = SharedState::new(vec![test_pr(1)], "bob".to_string());

        apply_navigation(KeyCode::Up, &mut state, &shared);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn navigation_down_increments_cursor() {
        let mut state = State::new();
        let shared = SharedState::new(vec![test_pr(1), test_pr(2)], "bob".to_string());

        apply_navigation(KeyCode::Down, &mut state, &shared);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn navigation_down_at_end_stays() {
        let mut state = State::new();
        state.cursor = 0;
        let shared = SharedState::new(vec![test_pr(1)], "bob".to_string());

        apply_navigation(KeyCode::Down, &mut state, &shared);
        assert_eq!(state.cursor, 0);
    }

    // ── apply_acknowledge tests ─────────────────────────────────────

    #[test]
    fn acknowledge_updates_pr_in_shared_state() {
        let mut shared = SharedState::new(vec![test_pr(1)], "bob".to_string());
        let mut state = State::new();
        let now = Utc.with_ymd_and_hms(2025, 6, 15, 12, 0, 0).unwrap();

        let result = apply_acknowledge(&mut shared, &mut state, 0, now);

        assert!(result.is_some());
        let pr = result.unwrap();
        assert_eq!(pr.last_acknowledged_at, Some(now));
        assert_eq!(shared.prs[0].last_acknowledged_at, Some(now));
    }

    #[test]
    fn acknowledge_out_of_bounds_returns_none() {
        let mut shared = SharedState::new(vec![test_pr(1)], "bob".to_string());
        let mut state = State::new();
        let now = Utc::now();

        let result = apply_acknowledge(&mut shared, &mut state, 99, now);

        assert!(result.is_none());
    }
}
