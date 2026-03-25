use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::db::DatabaseRepository;
use crate::tui::action::TuiAction;
use crate::tui::authors::State;
use crate::tui::navigation::{AuthorsPane, Screen};

/// Handle a key event for the Authors screen.
pub async fn handle_event(
    key_event: KeyEvent,
    state: &mut State,
    repo: &DatabaseRepository,
) -> anyhow::Result<TuiAction> {
    if key_event.kind != KeyEventKind::Press {
        return Ok(TuiAction::Continue);
    }

    let key_code = key_event.code;

    if state.loading {
        // When loading, only allow Esc and 'q' to exit
        match key_code {
            KeyCode::Esc | KeyCode::Char('q') => {
                return Ok(TuiAction::SwitchScreen(Screen::PrList));
            }
            _ => return Ok(TuiAction::Continue),
        }
    }

    // Not loading - handle all keys
    match key_code {
        KeyCode::Esc => {
            if !state.search_query.is_empty() {
                state.search_query.clear();
                state.tracked_cursor = 0;
                state.untracked_cursor = 0;
            } else {
                return Ok(TuiAction::SwitchScreen(Screen::PrList));
            }
        }

        KeyCode::Char('q') => {
            if state.search_query.is_empty() {
                return Ok(TuiAction::SwitchScreen(Screen::PrList));
            } else {
                state.search_query.push('q');
                state.tracked_cursor = 0;
                state.untracked_cursor = 0;
            }
        }

        KeyCode::Tab => {
            state.focus = match state.focus {
                AuthorsPane::Tracked => AuthorsPane::Untracked,
                AuthorsPane::Untracked => AuthorsPane::Tracked,
            };
        }

        KeyCode::Up | KeyCode::Char('k') => {
            let cursor = match state.focus {
                AuthorsPane::Tracked => &mut state.tracked_cursor,
                AuthorsPane::Untracked => &mut state.untracked_cursor,
            };
            if *cursor > 0 {
                *cursor -= 1;
            }
        }

        KeyCode::Down | KeyCode::Char('j') => {
            let filtered_len = match state.focus {
                AuthorsPane::Tracked => state.filtered_list(&state.tracked).len(),
                AuthorsPane::Untracked => state.filtered_list(&state.untracked).len(),
            };
            let cursor = match state.focus {
                AuthorsPane::Tracked => &mut state.tracked_cursor,
                AuthorsPane::Untracked => &mut state.untracked_cursor,
            };
            if filtered_len > 0 && *cursor + 1 < filtered_len {
                *cursor += 1;
            }
        }

        KeyCode::Enter | KeyCode::Char(' ') => match state.focus {
            AuthorsPane::Untracked => {
                let filtered = state.filtered_list(&state.untracked);
                let cursor = state.untracked_cursor;
                if let Some(&(orig_idx, _)) = filtered.get(cursor) {
                    let login = state.untracked.remove(orig_idx);
                    repo.save_tracked_author(&login).await?;
                    state.tracked.push(login);
                    state.tracked.sort();
                    state.search_query.clear();
                    state.clamp_cursors();
                }
            }
            AuthorsPane::Tracked => {
                let filtered = state.filtered_list(&state.tracked);
                let cursor = state.tracked_cursor;
                if let Some(&(orig_idx, _)) = filtered.get(cursor) {
                    let login = state.tracked.remove(orig_idx);
                    repo.delete_tracked_author(&login).await?;
                    state.untracked.push(login);
                    state.untracked.sort();
                    state.search_query.clear();
                    state.clamp_cursors();
                }
            }
        },

        KeyCode::Backspace => {
            state.search_query.pop();
            state.tracked_cursor = 0;
            state.untracked_cursor = 0;
        }

        KeyCode::Char(c) => {
            state.search_query.push(c);
            state.tracked_cursor = 0;
            state.untracked_cursor = 0;
        }

        _ => {}
    }

    Ok(TuiAction::Continue)
}
