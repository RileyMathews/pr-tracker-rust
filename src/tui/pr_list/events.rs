use crossterm::event::KeyCode;

use crate::db::DatabaseRepository;
use crate::models::PullRequest;
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

        KeyCode::Up | KeyCode::Char('k') => {
            let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
            state.ensure_cursor_in_range(filtered_indices.len());
            if state.cursor > 0 {
                state.cursor -= 1;
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Down | KeyCode::Char('j') => {
            let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
            state.ensure_cursor_in_range(filtered_indices.len());
            if state.cursor + 1 < filtered_indices.len() {
                state.cursor += 1;
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Enter | KeyCode::Char(' ') => {
            let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
            state.ensure_cursor_in_range(filtered_indices.len());
            if let Some(pr_index) = state.selected_index(&filtered_indices) {
                let pr = &shared.prs[pr_index];
                let _ = open::that(pr.url());
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Char('a') => {
            let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
            state.ensure_cursor_in_range(filtered_indices.len());
            if let Some(pr_index) = state.selected_index(&filtered_indices) {
                let mut pr = shared.prs[pr_index].clone();
                pr.last_acknowledged_at = Some(Utc::now());
                repo.save_pr(&pr).await?;
                shared.prs[pr_index] = pr;

                let updated_filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
                state.ensure_cursor_in_range(updated_filtered_indices.len());
            }
            Ok(EventResult::Continue)
        }

        KeyCode::Char('v') => {
            state.toggle_view();
            let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
            state.ensure_cursor_in_range(filtered_indices.len());
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
