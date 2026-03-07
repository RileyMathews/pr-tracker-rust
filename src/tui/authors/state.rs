use crossterm::event::KeyCode;
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

use crate::tui::navigation::{AuthorsPane, Screen};

/// State for the Authors from Teams screen.
pub struct State {
    /// List of tracked authors.
    pub tracked: Vec<String>,
    /// List of untracked authors.
    pub untracked: Vec<String>,
    /// Which pane is currently focused (Tracked or Untracked).
    pub focus: AuthorsPane,
    /// Cursor position in the tracked list.
    pub tracked_cursor: usize,
    /// Cursor position in the untracked list.
    pub untracked_cursor: usize,
    /// Current search filter query.
    pub search_query: String,
    /// Whether data is currently loading.
    pub loading: bool,
    /// Error message if loading failed.
    pub error: Option<String>,
}

/// Pure intent emitted by the Authors reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    SwitchScreen(Screen),
    TrackAuthor { login: String },
    UntrackAuthor { login: String },
}

impl State {
    /// Create a new Authors screen state with default values.
    /// Starts with loading=true, focus on Untracked, empty lists.
    pub fn new() -> Self {
        Self {
            tracked: Vec::new(),
            untracked: Vec::new(),
            focus: AuthorsPane::Untracked,
            tracked_cursor: 0,
            untracked_cursor: 0,
            search_query: String::new(),
            loading: true,
            error: None,
        }
    }

    /// Ensure cursors don't exceed list bounds.
    pub fn clamp_cursors(&mut self) {
        let tracked_len = self.filtered_list(&self.tracked).len();
        if tracked_len == 0 {
            self.tracked_cursor = 0;
        } else if self.tracked_cursor >= tracked_len {
            self.tracked_cursor = tracked_len - 1;
        }

        let untracked_len = self.filtered_list(&self.untracked).len();
        if untracked_len == 0 {
            self.untracked_cursor = 0;
        } else if self.untracked_cursor >= untracked_len {
            self.untracked_cursor = untracked_len - 1;
        }
    }

    /// Filter and score a list by search_query using fuzzy matching.
    ///
    /// Returns `(original_index, &login)` pairs, filtered and scored by search_query.
    /// When query is empty, returns all items in order sorted by fuzzy match score.
    pub fn filtered_list<'a>(&self, list: &'a [String]) -> Vec<(usize, &'a String)> {
        if self.search_query.is_empty() {
            return list.iter().enumerate().collect();
        }

        let matcher = SkimMatcherV2::default();
        let mut scored: Vec<(i64, usize, &String)> = list
            .iter()
            .enumerate()
            .filter_map(|(i, login)| {
                matcher
                    .fuzzy_match(login, &self.search_query)
                    .map(|score| (score, i, login))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, i, login)| (i, login)).collect()
    }

    /// Pure reducer for Authors key handling.
    pub fn reduce_key(&mut self, key_code: KeyCode) -> Action {
        if self.loading {
            return match key_code {
                KeyCode::Esc | KeyCode::Char('q') => Action::SwitchScreen(Screen::PrList),
                _ => Action::None,
            };
        }

        match key_code {
            KeyCode::Esc => {
                if self.search_query.is_empty() {
                    Action::SwitchScreen(Screen::PrList)
                } else {
                    self.search_query.clear();
                    self.tracked_cursor = 0;
                    self.untracked_cursor = 0;
                    Action::None
                }
            }

            KeyCode::Char('q') => {
                if self.search_query.is_empty() {
                    Action::SwitchScreen(Screen::PrList)
                } else {
                    self.search_query.push('q');
                    self.tracked_cursor = 0;
                    self.untracked_cursor = 0;
                    Action::None
                }
            }

            KeyCode::Tab => {
                self.focus = match self.focus {
                    AuthorsPane::Tracked => AuthorsPane::Untracked,
                    AuthorsPane::Untracked => AuthorsPane::Tracked,
                };
                Action::None
            }

            KeyCode::Up | KeyCode::Char('k') => {
                let cursor = match self.focus {
                    AuthorsPane::Tracked => &mut self.tracked_cursor,
                    AuthorsPane::Untracked => &mut self.untracked_cursor,
                };
                if *cursor > 0 {
                    *cursor -= 1;
                }
                Action::None
            }

            KeyCode::Down | KeyCode::Char('j') => {
                let filtered_len = match self.focus {
                    AuthorsPane::Tracked => self.filtered_list(&self.tracked).len(),
                    AuthorsPane::Untracked => self.filtered_list(&self.untracked).len(),
                };
                let cursor = match self.focus {
                    AuthorsPane::Tracked => &mut self.tracked_cursor,
                    AuthorsPane::Untracked => &mut self.untracked_cursor,
                };
                if filtered_len > 0 && *cursor + 1 < filtered_len {
                    *cursor += 1;
                }
                Action::None
            }

            KeyCode::Enter | KeyCode::Char(' ') => match self.focus {
                AuthorsPane::Untracked => {
                    let filtered = self.filtered_list(&self.untracked);
                    filtered
                        .get(self.untracked_cursor)
                        .map_or(Action::None, |(orig_idx, _)| Action::TrackAuthor {
                            login: self.untracked[*orig_idx].clone(),
                        })
                }
                AuthorsPane::Tracked => {
                    let filtered = self.filtered_list(&self.tracked);
                    filtered
                        .get(self.tracked_cursor)
                        .map_or(Action::None, |(orig_idx, _)| Action::UntrackAuthor {
                            login: self.tracked[*orig_idx].clone(),
                        })
                }
            },

            KeyCode::Backspace => {
                self.search_query.pop();
                self.tracked_cursor = 0;
                self.untracked_cursor = 0;
                Action::None
            }

            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.tracked_cursor = 0;
                self.untracked_cursor = 0;
                Action::None
            }

            _ => Action::None,
        }
    }

    /// Apply list transitions after a successful effect.
    pub fn apply_track_author(&mut self, login: &str) {
        if let Some(index) = self
            .untracked
            .iter()
            .position(|candidate| candidate == login)
        {
            let moved = self.untracked.remove(index);
            self.tracked.push(moved);
            self.tracked.sort();
            self.search_query.clear();
            self.clamp_cursors();
        }
    }

    /// Apply list transitions after a successful effect.
    pub fn apply_untrack_author(&mut self, login: &str) {
        if let Some(index) = self.tracked.iter().position(|candidate| candidate == login) {
            let moved = self.tracked.remove(index);
            self.untracked.push(moved);
            self.untracked.sort();
            self.search_query.clear();
            self.clamp_cursors();
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

    #[test]
    fn new_starts_with_loading_true() {
        let state = State::new();
        assert!(state.loading);
    }

    #[test]
    fn new_starts_with_focus_on_untracked() {
        let state = State::new();
        assert!(matches!(state.focus, AuthorsPane::Untracked));
    }

    #[test]
    fn clamp_cursors_uses_filtered_bounds() {
        let mut state = State::new();
        state.loading = false;
        state.tracked = vec!["alice".to_string(), "bob".to_string()];
        state.search_query = "bob".to_string();
        state.tracked_cursor = 5;

        state.clamp_cursors();

        assert_eq!(state.tracked_cursor, 0);
    }

    #[test]
    fn filtered_list_empty_query_returns_all() {
        let state = State::new();
        let list = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];

        let result = state.filtered_list(&list);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, 0);
        assert_eq!(result[1].0, 1);
        assert_eq!(result[2].0, 2);
    }

    #[test]
    fn reduce_key_tab_switches_focus() {
        let mut state = State::new();
        state.loading = false;

        let action = state.reduce_key(KeyCode::Tab);

        assert_eq!(action, Action::None);
        assert!(matches!(state.focus, AuthorsPane::Tracked));
    }

    #[test]
    fn reduce_key_down_stops_at_end_of_filtered_list() {
        let mut state = State::new();
        state.loading = false;
        state.untracked = vec!["alice".to_string(), "bob".to_string()];
        state.untracked_cursor = 1;

        let action = state.reduce_key(KeyCode::Down);

        assert_eq!(action, Action::None);
        assert_eq!(state.untracked_cursor, 1);
    }

    #[test]
    fn reduce_key_char_updates_query_and_resets_cursors() {
        let mut state = State::new();
        state.loading = false;
        state.tracked_cursor = 2;
        state.untracked_cursor = 3;

        let action = state.reduce_key(KeyCode::Char('a'));

        assert_eq!(action, Action::None);
        assert_eq!(state.search_query, "a");
        assert_eq!(state.tracked_cursor, 0);
        assert_eq!(state.untracked_cursor, 0);
    }

    #[test]
    fn reduce_key_enter_returns_track_intent_for_untracked_focus() {
        let mut state = State::new();
        state.loading = false;
        state.untracked = vec!["alice".to_string()];

        let action = state.reduce_key(KeyCode::Enter);

        assert_eq!(
            action,
            Action::TrackAuthor {
                login: "alice".to_string(),
            }
        );
    }

    #[test]
    fn reduce_key_enter_returns_untrack_intent_for_tracked_focus() {
        let mut state = State::new();
        state.loading = false;
        state.focus = AuthorsPane::Tracked;
        state.tracked = vec!["alice".to_string()];

        let action = state.reduce_key(KeyCode::Enter);

        assert_eq!(
            action,
            Action::UntrackAuthor {
                login: "alice".to_string(),
            }
        );
    }

    #[test]
    fn reduce_key_escape_clears_query_before_switching_screen() {
        let mut state = State::new();
        state.loading = false;
        state.search_query = "bob".to_string();

        let action = state.reduce_key(KeyCode::Esc);

        assert_eq!(action, Action::None);
        assert!(state.search_query.is_empty());
    }

    #[test]
    fn reduce_key_escape_switches_screen_when_query_empty() {
        let mut state = State::new();
        state.loading = false;

        let action = state.reduce_key(KeyCode::Esc);

        assert_eq!(action, Action::SwitchScreen(Screen::PrList));
    }

    #[test]
    fn apply_track_author_moves_login_between_lists() {
        let mut state = State::new();
        state.loading = false;
        state.untracked = vec!["alice".to_string()];

        state.apply_track_author("alice");

        assert!(state.untracked.is_empty());
        assert_eq!(state.tracked, vec!["alice".to_string()]);
    }

    #[test]
    fn apply_untrack_author_moves_login_between_lists() {
        let mut state = State::new();
        state.loading = false;
        state.tracked = vec!["alice".to_string()];

        state.apply_untrack_author("alice");

        assert!(state.tracked.is_empty());
        assert_eq!(state.untracked, vec!["alice".to_string()]);
    }
}
