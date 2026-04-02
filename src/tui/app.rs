use std::env;
use std::io;
use std::path::PathBuf;
use std::process::Command;
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
use crate::models::PullRequest;
use crate::pr_repository::{PrOwnerFilter, PrStatusFilter};
use crate::sync::{format_sync_progress, format_sync_summary};
use crate::tui::action::TuiAction;
use crate::tui::authors;
use crate::tui::navigation::Screen;
use crate::tui::pr_list;
use crate::tui::state::SharedState;
use crate::tui::tasks::{spawn_full_sync, spawn_teams_fetch, BackgroundJob, BackgroundMessage};

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

    let username = repo
        .get_user()
        .await?
        .map(|u| u.username)
        .unwrap_or_default();

    let dashboard = repo.get_pr_dashboard(&username).await?;
    let app_state = AppState::new(SharedState::new(dashboard, username));
    run_tui(app_state, &repo).await
}

/// Set up terminal and run the TUI inner loop.
async fn run_tui(app_state: AppState, repo: &DatabaseRepository) -> anyhow::Result<()> {
    let mut terminal = init_terminal()?;

    let result = run_tui_inner(&mut terminal, app_state, repo).await;

    // Always restore terminal, even on error
    let _ = restore_terminal();

    result
}

fn init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal() -> anyhow::Result<()> {
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn review_pr_in_octo_mode(pr: &PullRequest) {
    let home_dir = env::var_os("HOME").unwrap_or_else(|| "~".into());
    let repo_path = PathBuf::from(home_dir)
        .join("code")
        .join(pr.repository_name());
    let _ = Command::new("ghostty")
        .arg("+new-window")
        .arg(format!("--working-directory={}", repo_path.display()))
        .arg("-e")
        .arg("fish")
        .arg("-c")
        .arg(format!("pr_review {} {}", repo_path.display(), pr.number))
        .spawn();
}

/// Main TUI event loop.
async fn run_tui_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut app_state: AppState,
    repo: &DatabaseRepository,
) -> anyhow::Result<()> {
    let mut should_quit = false;
    let mut spinner_tick: usize = 0;
    let mut active_job: Option<BackgroundJob> = Some(BackgroundJob::FullSync);
    let (tx, mut rx) = mpsc::unbounded_channel::<BackgroundMessage>();

    app_state.pr_list.clear_sync_logs();
    spawn_full_sync(repo.clone(), tx.clone());

    while !should_quit {
        // Handle background messages
        while let Ok(message) = rx.try_recv() {
            match message {
                BackgroundMessage::SyncProgress(progress) => {
                    if let Some(line) = format_sync_progress(&progress) {
                        app_state.pr_list.push_sync_log(line);
                    }
                }
                BackgroundMessage::FullSyncFinished(result) => {
                    active_job = None;
                    spinner_tick = 0;

                    let summary = result?;
                    app_state
                        .pr_list
                        .push_sync_log(format_sync_summary(&summary));

                    // Reload dashboard from database
                    app_state.shared.dashboard =
                        repo.get_pr_dashboard(&app_state.shared.username).await?;
                    let status = match app_state.pr_list.view_mode {
                        crate::tui::navigation::ViewMode::Active => PrStatusFilter::Active,
                        crate::tui::navigation::ViewMode::Acknowledged => {
                            PrStatusFilter::Acknowledged
                        }
                    };
                    let tracked_len = app_state
                        .shared
                        .dashboard
                        .section(PrOwnerFilter::Tracked, status)
                        .len();
                    let mine_len = app_state
                        .shared
                        .dashboard
                        .section(PrOwnerFilter::Mine, status)
                        .len();
                    app_state.pr_list.clamp_cursors(tracked_len, mine_len);
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
                        app_state.shared.error = None;

                        match pr_list::events::handle_event(
                            key,
                            &mut app_state.pr_list,
                            &mut app_state.shared,
                            &active_job,
                            repo,
                            &tx,
                        )
                        .await?
                        {
                            TuiAction::Quit => should_quit = true,
                            TuiAction::SwitchScreen(screen) => {
                                app_state.current_screen = screen;
                                // Initialize Authors screen when switching to it
                                if screen == Screen::AuthorsFromTeams && active_job.is_none() {
                                    app_state.authors = authors::State::new();
                                    active_job = Some(BackgroundJob::TeamsFetch);
                                    spawn_teams_fetch(repo.clone(), tx.clone());
                                }
                            }
                            TuiAction::ReviewPr(pr) => review_pr_in_octo_mode(&pr),
                            TuiAction::StartJob(job) => {
                                active_job = Some(job);
                                spinner_tick = 0;
                            }
                            TuiAction::Continue => {}
                        }
                    }
                    Screen::AuthorsFromTeams => {
                        match authors::events::handle_event(key, &mut app_state.authors, repo)
                            .await?
                        {
                            TuiAction::Quit => should_quit = true,
                            TuiAction::SwitchScreen(screen) => {
                                app_state.current_screen = screen;
                            }
                            TuiAction::ReviewPr(_) => {}
                            TuiAction::StartJob(_) => {}
                            TuiAction::Continue => {}
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
