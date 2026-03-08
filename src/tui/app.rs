use std::io;
use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::db::DatabaseRepository;
use crate::tui::authors;
use crate::tui::navigation::Screen;
use crate::tui::pr_list;
use crate::tui::pr_list::events::EventResult;
use crate::tui::state::SharedState;
use crate::tui::tasks::{spawn_teams_fetch, BackgroundJob, BackgroundMessage};

/// Application state containing all screen states and shared data.
pub struct AppState {
    /// Shared state across all screens (PRs, username).
    pub shared: SharedState,
    /// State for the PR List screen.
    pub pr_list: pr_list::State,
    /// State for the Authors from Teams screen.
    pub authors: authors::State,
    /// Currently active screen.
    pub current_screen: Screen,
}

impl AppState {
    /// Create a new AppState with the given shared state.
    fn new(shared: SharedState) -> Self {
        Self {
            shared,
            pr_list: pr_list::State::new(),
            authors: authors::State::new(),
            current_screen: Screen::PrList,
        }
    }
}

/// Run the TUI application.
pub async fn run() -> anyhow::Result<()> {
    let db_path = crate::default_db_path();
    let repo = DatabaseRepository::connect(&db_path).await?;
    repo.apply_migrations().await?;

    let prs = repo.get_all_prs().await?;
    let username = repo
        .get_user()
        .await?
        .map(|u| u.username)
        .unwrap_or_default();

    let app_state = AppState::new(SharedState::new(prs, username));
    run_tui(app_state, &repo).await
}

/// Set up terminal and run the TUI inner loop.
async fn run_tui(app_state: AppState, repo: &DatabaseRepository) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_tui_inner(&mut terminal, app_state, repo).await;

    // Always restore terminal, even on error
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);

    result
}

/// Main TUI event loop.
async fn run_tui_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app_state: AppState,
    repo: &DatabaseRepository,
) -> anyhow::Result<()> {
    let mut should_quit = false;
    let mut spinner_tick: usize = 0;
    let mut active_job: Option<BackgroundJob> = None;
    let (tx, mut rx) = mpsc::unbounded_channel::<BackgroundMessage>();

    while !should_quit {
        // Handle background messages
        while let Ok(message) = rx.try_recv() {
            match message {
                BackgroundMessage::Progress => {
                    // Progress updates are handled implicitly by the spinner
                }
                BackgroundMessage::FullSyncFinished(result) => {
                    active_job = None;
                    spinner_tick = 0;

                    // Ignore the sync result for now (original behavior)
                    let _ = result;

                    // Reload PRs from database
                    app_state.shared.prs = repo.get_all_prs().await?;
                    let filtered_indices = app_state
                        .pr_list
                        .filtered_indices(&app_state.shared.prs, &app_state.shared.username);
                    app_state
                        .pr_list
                        .ensure_cursor_in_range(filtered_indices.len());
                }
                BackgroundMessage::TeamsFetchFinished(result) => {
                    active_job = None;
                    spinner_tick = 0;
                    match result {
                        Ok(payload) => {
                            app_state.authors.tracked = payload.tracked;
                            app_state.authors.untracked = payload.untracked;
                            app_state.authors.loading = false;
                            app_state.authors.error = None;
                            app_state.authors.clamp_cursors();
                        }
                        Err(e) => {
                            app_state.authors.loading = false;
                            app_state.authors.error = Some(e.to_string());
                        }
                    }
                }
            }
        }

        // Draw the current screen
        terminal.draw(|frame| match app_state.current_screen {
            Screen::PrList => {
                pr_list::render::draw(
                    frame,
                    &app_state.pr_list,
                    &app_state.shared,
                    active_job,
                    spinner_tick,
                );
            }
            Screen::AuthorsFromTeams => {
                authors::render::draw(frame, &app_state.authors, active_job, spinner_tick);
            }
        })?;

        // Update spinner if there's an active job
        if active_job.is_some() {
            spinner_tick = spinner_tick.wrapping_add(1);
        }

        // Poll for events with a 200ms timeout
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match app_state.current_screen {
                    Screen::PrList => {
                        match pr_list::events::handle_event(
                            key.code,
                            &mut app_state.pr_list,
                            &mut app_state.shared,
                            &active_job,
                            repo,
                            &tx,
                        )
                        .await?
                        {
                            EventResult::Quit => should_quit = true,
                            EventResult::SwitchScreen(screen) => {
                                app_state.current_screen = screen;
                                // Initialize Authors screen when switching to it
                                if screen == Screen::AuthorsFromTeams && active_job.is_none() {
                                    app_state.authors = authors::State::new();
                                    active_job = Some(BackgroundJob::TeamsFetch);
                                    spawn_teams_fetch(repo.clone(), tx.clone());
                                }
                            }
                            EventResult::Continue => {}
                        }
                    }
                    Screen::AuthorsFromTeams => {
                        match authors::events::handle_event(key.code, &mut app_state.authors, repo)
                            .await?
                        {
                            EventResult::Quit => should_quit = true,
                            EventResult::SwitchScreen(screen) => {
                                app_state.current_screen = screen;
                            }
                            EventResult::Continue => {}
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
