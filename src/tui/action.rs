use crate::models::PullRequest;
use crate::tui::navigation::Screen;
use crate::tui::tasks::BackgroundJob;

pub enum TuiAction {
    Continue,
    Quit,
    SwitchScreen(Screen),
    ReviewPr(PullRequest),
    StartJob(BackgroundJob),
}
