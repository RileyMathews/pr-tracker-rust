use crossterm::event::KeyCode;

use crate::db::DatabaseRepository;
use crate::tui::authors::State;
use crate::tui::navigation::{AuthorsPane, Screen};
use crate::tui::pr_list::events::EventResult;

/// A pure description of what side effect to perform on the Authors screen.
#[derive(Debug, PartialEq)]
pub enum AuthorAction {
    /// No side effect needed.
    None,
    /// Switch back to the PR list screen.
    SwitchToPrList,
    /// Track an author: move from untracked to tracked and persist.
    TrackAuthor {
        /// Index in the *original* untracked list (before filtering).
        original_index: usize,
    },
    /// Untrack an author: move from tracked to untracked and persist.
    UntrackAuthor {
        /// Index in the *original* tracked list (before filtering).
        original_index: usize,
    },
}

/// Pure function: given the current state and a key press, decide what action to take.
///
/// This function performs NO I/O. It only reads state and returns an action description.
pub fn resolve_event(key_code: KeyCode, state: &State) -> AuthorAction {
    if state.loading {
        match key_code {
            KeyCode::Esc | KeyCode::Char('q') => return AuthorAction::SwitchToPrList,
            _ => return AuthorAction::None,
        }
    }

    match key_code {
        KeyCode::Esc => {
            if state.search_query.is_empty() {
                AuthorAction::SwitchToPrList
            } else {
                AuthorAction::None // will be handled by apply_navigation
            }
        }

        KeyCode::Char('q') => {
            if state.search_query.is_empty() {
                AuthorAction::SwitchToPrList
            } else {
                AuthorAction::None // 'q' appended to search by apply_navigation
            }
        }

        KeyCode::Enter | KeyCode::Char(' ') => match state.focus {
            AuthorsPane::Untracked => {
                let filtered = state.filtered_list(&state.untracked);
                let cursor = state.untracked_cursor;
                match filtered.get(cursor) {
                    Some(&(orig_idx, _)) => AuthorAction::TrackAuthor {
                        original_index: orig_idx,
                    },
                    None => AuthorAction::None,
                }
            }
            AuthorsPane::Tracked => {
                let filtered = state.filtered_list(&state.tracked);
                let cursor = state.tracked_cursor;
                match filtered.get(cursor) {
                    Some(&(orig_idx, _)) => AuthorAction::UntrackAuthor {
                        original_index: orig_idx,
                    },
                    None => AuthorAction::None,
                }
            }
        },

        // All other keys (navigation, search) are handled purely by apply_navigation
        _ => AuthorAction::None,
    }
}

/// Pure function: apply navigation and search-related key events to state.
///
/// Handles cursor movement, tab switching, search input — no I/O.
pub fn apply_navigation(key_code: KeyCode, state: &mut State) {
    if state.loading {
        return;
    }

    match key_code {
        KeyCode::Esc => {
            if !state.search_query.is_empty() {
                state.search_query.clear();
                state.tracked_cursor = 0;
                state.untracked_cursor = 0;
            }
        }

        KeyCode::Char('q') => {
            if !state.search_query.is_empty() {
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
}

/// Pure function: apply a track action to the state.
///
/// Returns the login string for persistence by the imperative shell.
pub fn apply_track(state: &mut State, original_index: usize) -> Option<String> {
    if original_index >= state.untracked.len() {
        return None;
    }
    let login = state.untracked.remove(original_index);
    state.tracked.push(login.clone());
    state.tracked.sort();
    state.search_query.clear();
    state.clamp_cursors();
    Some(login)
}

/// Pure function: apply an untrack action to the state.
///
/// Returns the login string for persistence by the imperative shell.
pub fn apply_untrack(state: &mut State, original_index: usize) -> Option<String> {
    if original_index >= state.tracked.len() {
        return None;
    }
    let login = state.tracked.remove(original_index);
    state.untracked.push(login.clone());
    state.untracked.sort();
    state.search_query.clear();
    state.clamp_cursors();
    Some(login)
}

/// Imperative shell: execute an author action, performing all necessary I/O.
pub async fn execute_action(
    action: AuthorAction,
    state: &mut State,
    repo: &DatabaseRepository,
) -> anyhow::Result<EventResult> {
    match action {
        AuthorAction::None => Ok(EventResult::Continue),
        AuthorAction::SwitchToPrList => Ok(EventResult::SwitchScreen(Screen::PrList)),

        AuthorAction::TrackAuthor { original_index } => {
            if let Some(login) = apply_track(state, original_index) {
                repo.save_tracked_author(&login).await?;
            }
            Ok(EventResult::Continue)
        }

        AuthorAction::UntrackAuthor { original_index } => {
            if let Some(login) = apply_untrack(state, original_index) {
                repo.delete_tracked_author(&login).await?;
            }
            Ok(EventResult::Continue)
        }
    }
}

/// Top-level handler: resolves the event purely, then executes the resulting action.
pub async fn handle_event(
    key_code: KeyCode,
    state: &mut State,
    repo: &DatabaseRepository,
) -> anyhow::Result<EventResult> {
    // Pure: decide what action to take (before navigation modifies state)
    let action = resolve_event(key_code, state);

    // Pure: apply navigation state changes
    apply_navigation(key_code, state);

    // Impure: execute the action
    execute_action(action, state, repo).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::navigation::AuthorsPane;

    fn state_with_authors() -> State {
        let mut state = State::new();
        state.loading = false;
        state.tracked = vec!["alice".to_string(), "bob".to_string()];
        state.untracked = vec!["carol".to_string(), "dave".to_string()];
        state
    }

    // ── resolve_event tests ─────────────────────────────────────────

    #[test]
    fn resolve_quit_when_loading() {
        let mut state = State::new();
        state.loading = true;
        assert_eq!(
            resolve_event(KeyCode::Char('q'), &state),
            AuthorAction::SwitchToPrList
        );
        assert_eq!(
            resolve_event(KeyCode::Esc, &state),
            AuthorAction::SwitchToPrList
        );
    }

    #[test]
    fn resolve_other_key_when_loading() {
        let mut state = State::new();
        state.loading = true;
        assert_eq!(resolve_event(KeyCode::Enter, &state), AuthorAction::None);
    }

    #[test]
    fn resolve_esc_with_empty_search_exits() {
        let state = state_with_authors();
        assert_eq!(
            resolve_event(KeyCode::Esc, &state),
            AuthorAction::SwitchToPrList
        );
    }

    #[test]
    fn resolve_esc_with_search_returns_none() {
        let mut state = state_with_authors();
        state.search_query = "test".to_string();
        assert_eq!(resolve_event(KeyCode::Esc, &state), AuthorAction::None);
    }

    #[test]
    fn resolve_q_with_empty_search_exits() {
        let state = state_with_authors();
        assert_eq!(
            resolve_event(KeyCode::Char('q'), &state),
            AuthorAction::SwitchToPrList
        );
    }

    #[test]
    fn resolve_q_with_search_returns_none() {
        let mut state = state_with_authors();
        state.search_query = "test".to_string();
        assert_eq!(
            resolve_event(KeyCode::Char('q'), &state),
            AuthorAction::None
        );
    }

    #[test]
    fn resolve_enter_on_untracked_returns_track() {
        let mut state = state_with_authors();
        state.focus = AuthorsPane::Untracked;
        state.untracked_cursor = 0;
        let action = resolve_event(KeyCode::Enter, &state);
        assert_eq!(
            action,
            AuthorAction::TrackAuthor {
                original_index: 0
            }
        );
    }

    #[test]
    fn resolve_enter_on_tracked_returns_untrack() {
        let mut state = state_with_authors();
        state.focus = AuthorsPane::Tracked;
        state.tracked_cursor = 1;
        let action = resolve_event(KeyCode::Enter, &state);
        assert_eq!(
            action,
            AuthorAction::UntrackAuthor {
                original_index: 1
            }
        );
    }

    #[test]
    fn resolve_enter_on_empty_untracked_returns_none() {
        let mut state = state_with_authors();
        state.focus = AuthorsPane::Untracked;
        state.untracked = vec![];
        assert_eq!(resolve_event(KeyCode::Enter, &state), AuthorAction::None);
    }

    // ── apply_navigation tests ──────────────────────────────────────

    #[test]
    fn navigation_tab_toggles_focus() {
        let mut state = state_with_authors();
        state.focus = AuthorsPane::Tracked;
        apply_navigation(KeyCode::Tab, &mut state);
        assert_eq!(state.focus, AuthorsPane::Untracked);
        apply_navigation(KeyCode::Tab, &mut state);
        assert_eq!(state.focus, AuthorsPane::Tracked);
    }

    #[test]
    fn navigation_up_decrements_cursor() {
        let mut state = state_with_authors();
        state.focus = AuthorsPane::Tracked;
        state.tracked_cursor = 1;
        apply_navigation(KeyCode::Up, &mut state);
        assert_eq!(state.tracked_cursor, 0);
    }

    #[test]
    fn navigation_down_increments_cursor() {
        let mut state = state_with_authors();
        state.focus = AuthorsPane::Untracked;
        state.untracked_cursor = 0;
        apply_navigation(KeyCode::Down, &mut state);
        assert_eq!(state.untracked_cursor, 1);
    }

    #[test]
    fn navigation_char_appends_to_search() {
        let mut state = state_with_authors();
        apply_navigation(KeyCode::Char('x'), &mut state);
        assert_eq!(state.search_query, "x");
    }

    #[test]
    fn navigation_backspace_removes_from_search() {
        let mut state = state_with_authors();
        state.search_query = "abc".to_string();
        apply_navigation(KeyCode::Backspace, &mut state);
        assert_eq!(state.search_query, "ab");
    }

    #[test]
    fn navigation_noop_when_loading() {
        let mut state = State::new();
        state.loading = true;
        apply_navigation(KeyCode::Char('a'), &mut state);
        assert_eq!(state.search_query, "");
    }

    // ── apply_track / apply_untrack tests ───────────────────────────

    #[test]
    fn apply_track_moves_to_tracked() {
        let mut state = state_with_authors();
        let login = apply_track(&mut state, 0);
        assert_eq!(login, Some("carol".to_string()));
        assert!(state.tracked.contains(&"carol".to_string()));
        assert!(!state.untracked.contains(&"carol".to_string()));
    }

    #[test]
    fn apply_track_out_of_bounds_returns_none() {
        let mut state = state_with_authors();
        assert_eq!(apply_track(&mut state, 99), None);
    }

    #[test]
    fn apply_untrack_moves_to_untracked() {
        let mut state = state_with_authors();
        let login = apply_untrack(&mut state, 0);
        assert_eq!(login, Some("alice".to_string()));
        assert!(!state.tracked.contains(&"alice".to_string()));
        assert!(state.untracked.contains(&"alice".to_string()));
    }

    #[test]
    fn apply_untrack_out_of_bounds_returns_none() {
        let mut state = state_with_authors();
        assert_eq!(apply_untrack(&mut state, 99), None);
    }

    #[test]
    fn apply_track_clears_search_and_sorts() {
        let mut state = state_with_authors();
        state.search_query = "carol".to_string();
        apply_track(&mut state, 0);
        assert_eq!(state.search_query, "");
        let sorted: Vec<String> = state.tracked.clone();
        let mut expected = sorted.clone();
        expected.sort();
        assert_eq!(sorted, expected);
    }
}
