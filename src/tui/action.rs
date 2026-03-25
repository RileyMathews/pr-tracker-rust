use crate::tui::navigation::Screen;
use crate::tui::tasks::BackgroundJob;

pub enum TuiAction {
    Continue,
    Quit,
    SwitchScreen(Screen),
    ReviewPr(String),
    StartJob(BackgroundJob),
}
