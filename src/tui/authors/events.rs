use crossterm::event::KeyCode;

use crate::db::DatabaseRepository;
use crate::tui::authors::{Action, State};
use crate::tui::pr_list::events::EventResult;

/// Handle a key event for the Authors screen.
pub async fn handle_event(
    key_code: KeyCode,
    state: &mut State,
    repo: &DatabaseRepository,
) -> anyhow::Result<EventResult> {
    let action = state.reduce_key(key_code);

    match action {
        Action::None => Ok(EventResult::Continue),
        Action::SwitchScreen(screen) => Ok(EventResult::SwitchScreen(screen)),
        Action::TrackAuthor { login } => {
            repo.save_tracked_author(&login).await?;
            state.apply_track_author(&login);
            Ok(EventResult::Continue)
        }
        Action::UntrackAuthor { login } => {
            repo.delete_tracked_author(&login).await?;
            state.apply_untrack_author(&login);
            Ok(EventResult::Continue)
        }
    }
}
