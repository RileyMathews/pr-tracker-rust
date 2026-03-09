use crossterm::event::KeyCode;

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
}

/// Handle a key event for the PR List screen.
pub async fn handle_event(
    key_code: KeyCode,
    state: &mut State,
    shared: &mut SharedState,
    active_job: &Option<BackgroundJob>,
    repo: &DatabaseRepository,
    tx: &mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<EventResult> {
    match key_code {
        KeyCode::Char('q') => Ok(EventResult::Quit),

        KeyCode::Tab => {
            state.toggle_focus();
            let tracked_len = state.tracked_indices(&shared.prs, &shared.username).len();
            let mine_len = state.mine_indices(&shared.prs, &shared.username).len();
            state.clamp_cursors(tracked_len, mine_len);
            Ok(EventResult::Continue)
        }

        KeyCode::Up | KeyCode::Char('k') => {
            let filtered_len = state
                .focused_pane_indices(&shared.prs, &shared.username)
                .len();
            state.clamp_cursor(state.focus, filtered_len);
            let cursor = state.cursor_for_mut(state.focus);
            if *cursor > 0 {
                *cursor -= 1;
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Down | KeyCode::Char('j') => {
            let filtered_len = state
                .focused_pane_indices(&shared.prs, &shared.username)
                .len();
            state.clamp_cursor(state.focus, filtered_len);
            let cursor = state.cursor_for_mut(state.focus);
            if *cursor + 1 < filtered_len {
                *cursor += 1;
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(pr_index) = state.selected_index_for_focus(&shared.prs, &shared.username) {
                let pr = &shared.prs[pr_index];
                let _ = open::that(pr.url());
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Char('a') => {
            if let Some(pr_index) = state.selected_index_for_focus(&shared.prs, &shared.username) {
                let mut pr = shared.prs[pr_index].clone();
                pr.last_acknowledged_at = Some(Utc::now());
                repo.save_pr(&pr).await?;
                shared.prs[pr_index] = pr;

                let tracked_len = state.tracked_indices(&shared.prs, &shared.username).len();
                let mine_len = state.mine_indices(&shared.prs, &shared.username).len();
                state.clamp_cursors(tracked_len, mine_len);
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Char('v') => {
            state.toggle_view();
            let tracked_len = state.tracked_indices(&shared.prs, &shared.username).len();
            let mine_len = state.mine_indices(&shared.prs, &shared.username).len();
            state.clamp_cursors(tracked_len, mine_len);
            Ok(EventResult::Continue)
        }

        KeyCode::Char('s') => {
            if active_job.is_some() {
                return Ok(EventResult::Continue);
            }

            spawn_full_sync(repo.clone(), tx.clone());
            Ok(EventResult::Continue)
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
