use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::db::DatabaseRepository;
use crate::tui::navigation::Screen;
use crate::tui::pr_list::State;
use crate::tui::state::SharedState;
use crate::tui::tasks::{spawn_full_sync, BackgroundJob, BackgroundMessage};

use chrono::Utc;
use tokio::sync::mpsc;

/// Result of handling a key event.
pub enum EventResult {
    /// Continue running the TUI (no action needed).
    Continue,
    /// Quit the TUI application.
    Quit,
    /// Switch to a different screen.
    SwitchScreen(Screen),
    /// Launch PR review for the selected PR.
    ReviewPr(String),
    /// Start a background job and keep the TUI responsive.
    StartJob(BackgroundJob),
}

fn review_pr_url_for_event(
    key_event: KeyEvent,
    derived: &crate::tui::pr_list::state::DerivedPrList,
    shared: &SharedState,
) -> Option<String> {
    if key_event.kind != KeyEventKind::Press {
        return None;
    }

    if !(key_event.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key_event.code, KeyCode::Char('r') | KeyCode::Char('R')))
    {
        return None;
    }

    derived
        .selected_index_for_focus()
        .map(|pr_index| shared.prs[pr_index].url())
}

/// Handle a key event for the PR List screen.
pub async fn handle_event(
    key_event: KeyEvent,
    state: &mut State,
    shared: &mut SharedState,
    active_job: &Option<BackgroundJob>,
    repo: &DatabaseRepository,
    tx: &mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<EventResult> {
    let derived = state.derive(&shared.prs, &shared.username);

    if let Some(pr_url) = review_pr_url_for_event(key_event, &derived, shared) {
        return Ok(EventResult::ReviewPr(pr_url));
    }

    if key_event.kind != KeyEventKind::Press {
        return Ok(EventResult::Continue);
    }

    let key_code = key_event.code;

    match key_code {
        KeyCode::Char('q') => Ok(EventResult::Quit),

        KeyCode::Tab => {
            state.toggle_focus();
            let derived = state.derive(&shared.prs, &shared.username);
            state.clamp_to_derived(&derived);
            Ok(EventResult::Continue)
        }

        KeyCode::Up | KeyCode::Char('k') => {
            state.clamp_to_derived(&derived);
            let cursor = state.cursor_for_mut(state.focus);
            if *cursor > 0 {
                *cursor -= 1;
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Down | KeyCode::Char('j') => {
            state.clamp_to_derived(&derived);
            let filtered_len = derived.focused().len();
            let cursor = state.cursor_for_mut(state.focus);
            if *cursor + 1 < filtered_len {
                *cursor += 1;
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(pr_index) = derived.selected_index_for_focus() {
                let pr = &shared.prs[pr_index];
                let _ = open::that(pr.url());
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Char('a') => {
            if let Some(pr_index) = derived.selected_index_for_focus() {
                let mut pr = shared.prs[pr_index].clone();
                pr.last_acknowledged_at = Some(Utc::now());
                repo.save_pr(&pr).await?;
                shared.prs[pr_index] = pr;

                let derived = state.derive(&shared.prs, &shared.username);
                state.clamp_to_derived(&derived);
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Char('v') => {
            state.toggle_view();
            let derived = state.derive(&shared.prs, &shared.username);
            state.clamp_to_derived(&derived);
            Ok(EventResult::Continue)
        }

        KeyCode::Char('s') => {
            if active_job.is_some() {
                return Ok(EventResult::Continue);
            }

            state.clear_sync_logs();
            spawn_full_sync(repo.clone(), tx.clone());
            Ok(EventResult::StartJob(BackgroundJob::FullSync))
        }

        KeyCode::Char('t') => {
            if active_job.is_none() {
                Ok(EventResult::SwitchScreen(Screen::AuthorsFromTeams))
            } else {
                Ok(EventResult::Continue)
            }
        }

        _ => Ok(EventResult::Continue),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ApprovalStatus, CiStatus, PullRequest};
    use chrono::DateTime;

    fn test_pr(number: i64, author: &str) -> PullRequest {
        PullRequest {
            number,
            title: "Test PR".to_string(),
            repository: "owner/repo".to_string(),
            author: author.to_string(),
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

    #[test]
    fn review_pr_url_for_event_returns_selected_pr_url_for_ctrl_r() {
        let state = State::new();
        let shared = SharedState::new(vec![test_pr(42, "bob")], "alice".to_string());
        let derived = state.derive(&shared.prs, &shared.username);
        let key_event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);

        assert_eq!(
            review_pr_url_for_event(key_event, &derived, &shared),
            Some("https://github.com/owner/repo/pull/42".to_string())
        );
    }

    #[test]
    fn review_pr_url_for_event_ignores_non_press_events() {
        let state = State::new();
        let shared = SharedState::new(vec![test_pr(42, "bob")], "alice".to_string());
        let derived = state.derive(&shared.prs, &shared.username);
        let mut key_event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        key_event.kind = KeyEventKind::Release;

        assert_eq!(review_pr_url_for_event(key_event, &derived, &shared), None);
    }

    #[test]
    fn review_pr_url_for_event_returns_none_without_selection() {
        let state = State::new();
        let shared = SharedState::new(vec![], "alice".to_string());
        let derived = state.derive(&shared.prs, &shared.username);
        let key_event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);

        assert_eq!(review_pr_url_for_event(key_event, &derived, &shared), None);
    }
}
