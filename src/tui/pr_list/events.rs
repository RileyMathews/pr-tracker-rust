use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::db::DatabaseRepository;
use crate::pr_repository::{selected_pr_index, PrOwnerFilter, PrStatusFilter};
use crate::tui::action::TuiAction;
use crate::tui::navigation::Screen;
use crate::tui::pr_list::state::clamp_cursor;
use crate::tui::pr_list::State;
use crate::tui::state::SharedState;
use crate::tui::tasks::{spawn_full_sync, BackgroundJob, BackgroundMessage};

use chrono::Utc;
use tokio::sync::mpsc;

fn indices_for<'a>(state: &State, shared: &'a SharedState, owner: PrOwnerFilter) -> &'a [usize] {
    shared.dashboard.section(
        owner,
        match state.view_mode {
            crate::tui::navigation::ViewMode::Active => PrStatusFilter::Active,
            crate::tui::navigation::ViewMode::Acknowledged => PrStatusFilter::Acknowledged,
        },
    )
}

fn focused_indices<'a>(state: &State, shared: &'a SharedState) -> &'a [usize] {
    match state.focus {
        crate::tui::navigation::PrPane::Tracked => {
            indices_for(state, shared, PrOwnerFilter::Tracked)
        }
        crate::tui::navigation::PrPane::Mine => indices_for(state, shared, PrOwnerFilter::Mine),
    }
}

fn selected_index_for_focus(state: &State, shared: &SharedState) -> Option<usize> {
    let indices = focused_indices(state, shared);
    let cursor = match state.focus {
        crate::tui::navigation::PrPane::Tracked => state.tracked_cursor,
        crate::tui::navigation::PrPane::Mine => state.mine_cursor,
    };
    selected_pr_index(indices, cursor)
}

fn review_pr_url_for_event(
    key_event: KeyEvent,
    state: &State,
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

    selected_index_for_focus(state, shared).map(|pr_index| shared.dashboard.prs[pr_index].url())
}

/// Handle a key event for the PR List screen.
pub async fn handle_event(
    key_event: KeyEvent,
    state: &mut State,
    shared: &mut SharedState,
    active_job: &Option<BackgroundJob>,
    repo: &DatabaseRepository,
    tx: &mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<TuiAction> {
    if let Some(pr_url) = review_pr_url_for_event(key_event, state, shared) {
        return Ok(TuiAction::ReviewPr(pr_url));
    }

    if key_event.kind != KeyEventKind::Press {
        return Ok(TuiAction::Continue);
    }

    let key_code = key_event.code;

    match key_code {
        KeyCode::Char('q') => Ok(TuiAction::Quit),

        KeyCode::Tab => {
            state.toggle_focus();
            let tracked_len = indices_for(state, shared, PrOwnerFilter::Tracked).len();
            let mine_len = indices_for(state, shared, PrOwnerFilter::Mine).len();
            state.clamp_cursors(tracked_len, mine_len);
            Ok(TuiAction::Continue)
        }

        KeyCode::Up | KeyCode::Char('k') => {
            let cursor = state.cursor_for_mut(state.focus);
            if *cursor > 0 {
                *cursor -= 1;
            }
            Ok(TuiAction::Continue)
        }

        KeyCode::Down | KeyCode::Char('j') => {
            let filtered_len = focused_indices(state, shared).len();
            let cursor = state.cursor_for_mut(state.focus);
            let next = cursor.saturating_add(1);
            if filtered_len > 0 {
                *cursor = clamp_cursor(next, filtered_len);
            }
            Ok(TuiAction::Continue)
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(pr_index) = selected_index_for_focus(state, shared) {
                let pr = &shared.dashboard.prs[pr_index];
                let _ = open::that(pr.url());
            }
            Ok(TuiAction::Continue)
        }

        KeyCode::Char('a') => {
            if let Some(pr_index) = selected_index_for_focus(state, shared) {
                let mut pr = shared.dashboard.prs[pr_index].clone();
                pr.last_acknowledged_at = Some(Utc::now());
                repo.save_pr(&pr).await?;
                shared.dashboard = repo.get_pr_dashboard(&shared.username).await?;

                let tracked_len = indices_for(state, shared, PrOwnerFilter::Tracked).len();
                let mine_len = indices_for(state, shared, PrOwnerFilter::Mine).len();
                state.clamp_cursors(tracked_len, mine_len);
            }
            Ok(TuiAction::Continue)
        }

        KeyCode::Char('v') => {
            state.toggle_view();
            Ok(TuiAction::Continue)
        }

        KeyCode::Char('s') => {
            if active_job.is_some() {
                return Ok(TuiAction::Continue);
            }

            state.clear_sync_logs();
            spawn_full_sync(repo.clone(), tx.clone());
            Ok(TuiAction::StartJob(BackgroundJob::FullSync))
        }

        KeyCode::Char('t') => {
            if active_job.is_none() {
                Ok(TuiAction::SwitchScreen(Screen::AuthorsFromTeams))
            } else {
                Ok(TuiAction::Continue)
            }
        }

        _ => Ok(TuiAction::Continue),
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
        let shared = SharedState::new(
            crate::pr_repository::build_pr_dashboard(vec![test_pr(42, "bob")], "alice"),
            "alice".to_string(),
        );
        let key_event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);

        assert_eq!(
            review_pr_url_for_event(key_event, &state, &shared),
            Some("https://github.com/owner/repo/pull/42".to_string())
        );
    }

    #[test]
    fn review_pr_url_for_event_ignores_non_press_events() {
        let state = State::new();
        let shared = SharedState::new(
            crate::pr_repository::build_pr_dashboard(vec![test_pr(42, "bob")], "alice"),
            "alice".to_string(),
        );
        let mut key_event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);
        key_event.kind = KeyEventKind::Release;

        assert_eq!(review_pr_url_for_event(key_event, &state, &shared), None);
    }

    #[test]
    fn review_pr_url_for_event_returns_none_without_selection() {
        let state = State::new();
        let shared = SharedState::new(
            crate::pr_repository::build_pr_dashboard(vec![], "alice"),
            "alice".to_string(),
        );
        let key_event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL);

        assert_eq!(review_pr_url_for_event(key_event, &state, &shared), None);
    }
}
