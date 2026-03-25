use crate::tui::navigation::{PrPane, ViewMode};

const MAX_SYNC_LOG_LINES: usize = 256;

pub fn clamp_cursor(cursor: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        cursor.min(len - 1)
    }
}

/// State for the PR List screen.
pub struct State {
    /// Which pane is currently focused.
    pub focus: PrPane,
    /// Cursor position in the tracked-authors pane.
    pub tracked_cursor: usize,
    /// Cursor position in the authored-by-me pane.
    pub mine_cursor: usize,
    /// Current view mode (Active or Acknowledged).
    pub view_mode: ViewMode,
    /// Recent sync log lines shown while a sync is running.
    pub sync_logs: Vec<String>,
}

impl State {
    /// Create a new PR List state with default values.
    pub fn new() -> Self {
        Self {
            focus: PrPane::Tracked,
            tracked_cursor: 0,
            mine_cursor: 0,
            view_mode: ViewMode::Active,
            sync_logs: Vec::new(),
        }
    }

    pub fn clear_sync_logs(&mut self) {
        self.sync_logs.clear();
    }

    pub fn push_sync_log(&mut self, line: impl Into<String>) {
        self.sync_logs.push(line.into());
        if self.sync_logs.len() > MAX_SYNC_LOG_LINES {
            let overflow = self.sync_logs.len() - MAX_SYNC_LOG_LINES;
            self.sync_logs.drain(0..overflow);
        }
    }

    pub fn cursor_for_mut(&mut self, pane: PrPane) -> &mut usize {
        match pane {
            PrPane::Tracked => &mut self.tracked_cursor,
            PrPane::Mine => &mut self.mine_cursor,
        }
    }

    pub fn clamp_cursors(&mut self, tracked_len: usize, mine_len: usize) {
        self.tracked_cursor = clamp_cursor(self.tracked_cursor, tracked_len);
        self.mine_cursor = clamp_cursor(self.mine_cursor, mine_len);
    }

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            PrPane::Tracked => PrPane::Mine,
            PrPane::Mine => PrPane::Tracked,
        };
    }

    /// Toggle between Active and Acknowledged view modes.
    /// Resets both cursors to 0 when toggling.
    pub fn toggle_view(&mut self) {
        self.view_mode = self.view_mode.toggle();
        self.tracked_cursor = 0;
        self.mine_cursor = 0;
    }

    /// Return the label for the current view mode.
    pub fn view_label(&self) -> &'static str {
        self.view_mode.label()
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
    fn new_starts_with_tracked_focus() {
        let state = State::new();
        assert!(matches!(state.focus, PrPane::Tracked));
    }

    #[test]
    fn new_starts_with_zero_cursors() {
        let state = State::new();
        assert_eq!(state.tracked_cursor, 0);
        assert_eq!(state.mine_cursor, 0);
    }

    #[test]
    fn new_starts_with_empty_sync_logs() {
        let state = State::new();
        assert!(state.sync_logs.is_empty());
    }

    #[test]
    fn push_sync_log_keeps_recent_entries() {
        let mut state = State::new();

        for index in 0..300 {
            state.push_sync_log(format!("line {index}"));
        }

        assert_eq!(state.sync_logs.len(), 256);
        assert_eq!(state.sync_logs.first(), Some(&"line 44".to_string()));
        assert_eq!(state.sync_logs.last(), Some(&"line 299".to_string()));
    }

    #[test]
    fn toggle_focus_switches_to_mine() {
        let mut state = State::new();
        state.toggle_focus();
        assert!(matches!(state.focus, PrPane::Mine));
    }

    #[test]
    fn toggle_view_resets_both_cursors() {
        let mut state = State::new();
        state.tracked_cursor = 2;
        state.mine_cursor = 3;
        state.toggle_view();
        assert_eq!(state.tracked_cursor, 0);
        assert_eq!(state.mine_cursor, 0);
    }

    #[test]
    fn clamp_cursors_clamps_each_pane_independently() {
        let mut state = State::new();
        state.tracked_cursor = 10;
        state.mine_cursor = 4;

        state.clamp_cursors(2, 0);

        assert_eq!(state.tracked_cursor, 1);
        assert_eq!(state.mine_cursor, 0);
    }

    #[test]
    fn clamp_cursor_returns_zero_for_empty_lists() {
        assert_eq!(clamp_cursor(10, 0), 0);
    }

    #[test]
    fn clamp_cursor_clamps_to_last_valid_index() {
        assert_eq!(clamp_cursor(10, 2), 1);
    }
}
