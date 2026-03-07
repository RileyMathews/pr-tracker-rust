use chrono::Utc;
use crossterm::event::KeyCode;
use tokio::sync::mpsc;

use crate::db::DatabaseRepository;
use crate::models::PullRequest;
use crate::tui::navigation::Screen;
use crate::tui::pr_list::{Action, State};
use crate::tui::state::SharedState;
use crate::tui::tasks::{spawn_full_sync, BackgroundJob, BackgroundMessage};

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
    let action = state.reduce_key(key_code, shared, active_job.is_some());

    match action {
        Action::None => Ok(EventResult::Continue),
        Action::Quit => Ok(EventResult::Quit),
        Action::SwitchScreen(screen) => Ok(EventResult::SwitchScreen(screen)),
        Action::OpenSelectedPr { pr_index } => {
            if let Some(pr) = shared.prs.get(pr_index) {
                let _ = open::that(pr.url());
            }
            Ok(EventResult::Continue)
        }
        Action::AcknowledgeSelectedPr { pr_index } => {
            if let Some(selected_pr) = shared.prs.get(pr_index).cloned() {
                let mut pr = selected_pr;
                pr.last_acknowledged_at = Some(Utc::now());
                repo.save_pr(&pr).await?;
                shared.prs[pr_index] = pr;

                let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
                state.ensure_cursor_in_range(filtered_indices.len());
            }
            Ok(EventResult::Continue)
        }
        Action::TriggerSync => {
            spawn_full_sync(repo.clone(), tx.clone());
            Ok(EventResult::Continue)
        }
    }
}

/// Helper function to get the currently selected PR.
/// Used when acknowledging or opening PRs.
pub fn get_selected_pr<'a>(state: &State, shared: &'a SharedState) -> Option<&'a PullRequest> {
    let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
    state
        .selected_index(&filtered_indices)
        .and_then(|index| shared.prs.get(index))
}
