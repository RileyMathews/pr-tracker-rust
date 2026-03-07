/// Which screen is currently being displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    PrList,
    AuthorsFromTeams,
}

/// Which view mode for the PR list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Active,
    Acknowledged,
}

impl ViewMode {
    /// Toggle between Active and Acknowledged view modes.
    pub fn toggle(self) -> Self {
        match self {
            ViewMode::Active => ViewMode::Acknowledged,
            ViewMode::Acknowledged => ViewMode::Active,
        }
    }

    /// Return the label for the current view mode.
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Active => "active",
            ViewMode::Acknowledged => "acknowledged",
        }
    }
}

/// Which pane is focused on the Authors screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorsPane {
    Tracked,
    Untracked,
}
