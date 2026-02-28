use std::io;
use std::time::Duration;

use chrono::Utc;
use crossterm::event::{self, Event, KeyCode};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::db::DatabaseRepository;
use crate::github::GitHubClient;
use crate::models::{CiStatus, PullRequest};
use crate::sync::{
    refresh_existing_pull_requests_with_progress, sync_all_tracked_with_progress,
    QuickRefreshSummary, SyncRunSummary,
};

struct Model {
    prs: Vec<PullRequest>,
    cursor: usize,
    view_mode: ViewMode,
    screen: Screen,
    authors_screen: AuthorsScreenState,
}

#[derive(Clone, Copy)]
enum ViewMode {
    Active,
    Acknowledged,
}

#[derive(Clone, Copy)]
enum BackgroundJob {
    FullSync,
    QuickRefresh,
    TeamsFetch,
}

enum BackgroundMessage {
    Progress,
    FullSyncFinished(anyhow::Result<SyncRunSummary>),
    QuickRefreshFinished(anyhow::Result<QuickRefreshSummary>),
    TeamsFetchFinished(anyhow::Result<TeamsPayload>),
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    PrList,
    AuthorsFromTeams,
}

#[derive(Clone, Copy, PartialEq)]
enum AuthorsPane {
    Tracked,
    Untracked,
}

struct TeamsPayload {
    tracked: Vec<String>,
    untracked: Vec<String>,
}

struct AuthorsScreenState {
    tracked: Vec<String>,
    untracked: Vec<String>,
    focus: AuthorsPane,
    tracked_cursor: usize,
    untracked_cursor: usize,
    loading: bool,
    error: Option<String>,
    search_query: String,
}

impl AuthorsScreenState {
    fn new() -> Self {
        Self {
            tracked: Vec::new(),
            untracked: Vec::new(),
            focus: AuthorsPane::Untracked,
            tracked_cursor: 0,
            untracked_cursor: 0,
            loading: true,
            error: None,
            search_query: String::new(),
        }
    }

    fn clamp_cursors(&mut self) {
        if self.tracked.is_empty() {
            self.tracked_cursor = 0;
        } else if self.tracked_cursor >= self.tracked.len() {
            self.tracked_cursor = self.tracked.len() - 1;
        }
        if self.untracked.is_empty() {
            self.untracked_cursor = 0;
        } else if self.untracked_cursor >= self.untracked.len() {
            self.untracked_cursor = self.untracked.len() - 1;
        }
    }

    fn filtered_list<'a>(&self, list: &'a [String]) -> Vec<(usize, &'a String)> {
        // Returns (original_index, &login) pairs, filtered and scored by search_query.
        // When query is empty, returns all items in order.
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
}

impl Model {
    fn new(prs: Vec<PullRequest>) -> Self {
        Self {
            prs,
            cursor: 0,
            view_mode: ViewMode::Active,
            screen: Screen::PrList,
            authors_screen: AuthorsScreenState::new(),
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        self.prs
            .iter()
            .enumerate()
            .filter_map(|(index, pr)| {
                let include = match self.view_mode {
                    ViewMode::Active => !pr.is_acknowledged(),
                    ViewMode::Acknowledged => pr.is_acknowledged(),
                };

                if include {
                    Some(index)
                } else {
                    None
                }
            })
            .collect()
    }

    fn selected_index(&self, filtered_indices: &[usize]) -> Option<usize> {
        filtered_indices.get(self.cursor).copied()
    }

    fn ensure_cursor_in_range(&mut self, len: usize) {
        if len == 0 {
            self.cursor = 0;
            return;
        }

        if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    fn toggle_view(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Active => ViewMode::Acknowledged,
            ViewMode::Acknowledged => ViewMode::Active,
        };
        self.cursor = 0;
    }

    fn view_label(&self) -> &'static str {
        match self.view_mode {
            ViewMode::Active => "active",
            ViewMode::Acknowledged => "acknowledged",
        }
    }
}

pub async fn run() -> anyhow::Result<()> {
    let db_path = crate::default_db_path();
    let repo = DatabaseRepository::connect(&db_path).await?;
    repo.apply_migrations().await?;

    let prs = repo.get_all_prs().await?;
    run_tui(Model::new(prs), &repo).await
}

async fn run_tui(mut model: Model, repo: &DatabaseRepository) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_tui_inner(&mut terminal, &mut model, repo).await;

    // Always restore terminal, even on error
    let _ = disable_raw_mode();
    let _ = io::stdout().execute(LeaveAlternateScreen);

    result
}

async fn run_tui_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    model: &mut Model,
    repo: &DatabaseRepository,
) -> anyhow::Result<()> {
    let mut should_quit = false;
    let mut spinner_tick = 0usize;
    let mut active_job: Option<BackgroundJob> = None;
    let (tx, mut rx) = mpsc::unbounded_channel::<BackgroundMessage>();

    while !should_quit {
        while let Ok(message) = rx.try_recv() {
            match message {
                BackgroundMessage::Progress => {}
                BackgroundMessage::FullSyncFinished(result) => {
                    active_job = None;
                    spinner_tick = 0;

                    let _ = result;

                    model.prs = repo.get_all_prs().await?;
                    let filtered_indices = model.filtered_indices();
                    model.ensure_cursor_in_range(filtered_indices.len());
                }
                BackgroundMessage::QuickRefreshFinished(result) => {
                    active_job = None;
                    spinner_tick = 0;

                    let _ = result;

                    model.prs = repo.get_all_prs().await?;
                    let filtered_indices = model.filtered_indices();
                    model.ensure_cursor_in_range(filtered_indices.len());
                }
                BackgroundMessage::TeamsFetchFinished(result) => {
                    active_job = None;
                    spinner_tick = 0;
                    match result {
                        Ok(payload) => {
                            model.authors_screen.tracked = payload.tracked;
                            model.authors_screen.untracked = payload.untracked;
                            model.authors_screen.loading = false;
                            model.authors_screen.error = None;
                            model.authors_screen.clamp_cursors();
                        }
                        Err(e) => {
                            model.authors_screen.loading = false;
                            model.authors_screen.error = Some(e.to_string());
                        }
                    }
                }
            }
        }

        terminal.draw(|frame| draw(frame, model, active_job, spinner_tick))?;
        if active_job.is_some() {
            spinner_tick = spinner_tick.wrapping_add(1);
        }

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match model.screen {
                    Screen::PrList => {
                        match key.code {
                            KeyCode::Char('q') => should_quit = true,
                            KeyCode::Up | KeyCode::Char('k') => {
                                let filtered_indices = model.filtered_indices();
                                model.ensure_cursor_in_range(filtered_indices.len());
                                if model.cursor > 0 {
                                    model.cursor -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let filtered_indices = model.filtered_indices();
                                model.ensure_cursor_in_range(filtered_indices.len());
                                if model.cursor + 1 < filtered_indices.len() {
                                    model.cursor += 1;
                                }
                            }
                            KeyCode::Enter | KeyCode::Char(' ') => {
                                let filtered_indices = model.filtered_indices();
                                model.ensure_cursor_in_range(filtered_indices.len());
                                if let Some(pr_index) = model.selected_index(&filtered_indices) {
                                    let pr = &model.prs[pr_index];
                                    let _ = open::that(pr.url());
                                }
                            }
                            KeyCode::Char('a') => {
                                let filtered_indices = model.filtered_indices();
                                model.ensure_cursor_in_range(filtered_indices.len());
                                if let Some(pr_index) = model.selected_index(&filtered_indices) {
                                    let mut pr = model.prs[pr_index].clone();
                                    pr.last_acknowledged_at = Some(Utc::now());
                                    repo.save_pr(&pr).await?;
                                    model.prs[pr_index] = pr;

                                    let updated_filtered_indices = model.filtered_indices();
                                    model.ensure_cursor_in_range(updated_filtered_indices.len());
                                }
                            }
                            KeyCode::Char('v') => {
                                model.toggle_view();
                                let filtered_indices = model.filtered_indices();
                                model.ensure_cursor_in_range(filtered_indices.len());
                            }
                            KeyCode::Char('s') => {
                                if active_job.is_some() {
                                    continue;
                                }

                                active_job = Some(BackgroundJob::FullSync);
                                spawn_full_sync(repo.clone(), tx.clone());
                            }
                            KeyCode::Char('r') => {
                                if active_job.is_some() {
                                    continue;
                                }

                                active_job = Some(BackgroundJob::QuickRefresh);
                                spawn_quick_refresh(repo.clone(), tx.clone());
                            }
                            KeyCode::Char('t') => {
                                if active_job.is_none() {
                                    model.screen = Screen::AuthorsFromTeams;
                                    model.authors_screen = AuthorsScreenState::new();
                                    active_job = Some(BackgroundJob::TeamsFetch);
                                    spawn_teams_fetch(repo.clone(), tx.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                    Screen::AuthorsFromTeams => {
                        if model.authors_screen.loading {
                            if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                                model.screen = Screen::PrList;
                            }
                        } else {
                            match key.code {
                                KeyCode::Esc => {
                                    if !model.authors_screen.search_query.is_empty() {
                                        model.authors_screen.search_query.clear();
                                        model.authors_screen.tracked_cursor = 0;
                                        model.authors_screen.untracked_cursor = 0;
                                    } else {
                                        model.screen = Screen::PrList;
                                    }
                                }
                                KeyCode::Char('q') => {
                                    if model.authors_screen.search_query.is_empty() {
                                        model.screen = Screen::PrList;
                                    } else {
                                        model.authors_screen.search_query.push('q');
                                        model.authors_screen.tracked_cursor = 0;
                                        model.authors_screen.untracked_cursor = 0;
                                    }
                                }
                                KeyCode::Tab => {
                                    model.authors_screen.focus =
                                        match model.authors_screen.focus {
                                            AuthorsPane::Tracked => AuthorsPane::Untracked,
                                            AuthorsPane::Untracked => AuthorsPane::Tracked,
                                        };
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let cursor = match model.authors_screen.focus {
                                        AuthorsPane::Tracked => {
                                            &mut model.authors_screen.tracked_cursor
                                        }
                                        AuthorsPane::Untracked => {
                                            &mut model.authors_screen.untracked_cursor
                                        }
                                    };
                                    if *cursor > 0 {
                                        *cursor -= 1;
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let filtered_len = match model.authors_screen.focus {
                                        AuthorsPane::Tracked => {
                                            model.authors_screen.filtered_list(&model.authors_screen.tracked).len()
                                        }
                                        AuthorsPane::Untracked => {
                                            model.authors_screen.filtered_list(&model.authors_screen.untracked).len()
                                        }
                                    };
                                    let cursor = match model.authors_screen.focus {
                                        AuthorsPane::Tracked => {
                                            &mut model.authors_screen.tracked_cursor
                                        }
                                        AuthorsPane::Untracked => {
                                            &mut model.authors_screen.untracked_cursor
                                        }
                                    };
                                    if filtered_len > 0 && *cursor + 1 < filtered_len {
                                        *cursor += 1;
                                    }
                                }
                                KeyCode::Enter | KeyCode::Char(' ') => {
                                    match model.authors_screen.focus {
                                        AuthorsPane::Untracked => {
                                            let filtered = model.authors_screen.filtered_list(&model.authors_screen.untracked);
                                            let cursor = model.authors_screen.untracked_cursor;
                                            if let Some(&(orig_idx, _)) = filtered.get(cursor) {
                                                let login = model.authors_screen.untracked.remove(orig_idx);
                                                repo.save_tracked_author(&login).await?;
                                                model.authors_screen.tracked.push(login);
                                                model.authors_screen.tracked.sort();
                                                model.authors_screen.search_query.clear();
                                                model.authors_screen.clamp_cursors();
                                            }
                                        }
                                        AuthorsPane::Tracked => {
                                            let filtered = model.authors_screen.filtered_list(&model.authors_screen.tracked);
                                            let cursor = model.authors_screen.tracked_cursor;
                                            if let Some(&(orig_idx, _)) = filtered.get(cursor) {
                                                let login = model.authors_screen.tracked.remove(orig_idx);
                                                repo.delete_tracked_author(&login).await?;
                                                model.authors_screen.untracked.push(login);
                                                model.authors_screen.untracked.sort();
                                                model.authors_screen.search_query.clear();
                                                model.authors_screen.clamp_cursors();
                                            }
                                        }
                                    }
                                }
                                KeyCode::Backspace => {
                                    model.authors_screen.search_query.pop();
                                    model.authors_screen.tracked_cursor = 0;
                                    model.authors_screen.untracked_cursor = 0;
                                }
                                KeyCode::Char(c) => {
                                    model.authors_screen.search_query.push(c);
                                    model.authors_screen.tracked_cursor = 0;
                                    model.authors_screen.untracked_cursor = 0;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn draw(
    frame: &mut ratatui::Frame<'_>,
    model: &Model,
    active_job: Option<BackgroundJob>,
    spinner_tick: usize,
) {
    match model.screen {
        Screen::PrList => draw_pr_list(frame, model, active_job, spinner_tick),
        Screen::AuthorsFromTeams => draw_authors_screen(frame, model, active_job, spinner_tick),
    }
}

fn draw_pr_list(
    frame: &mut ratatui::Frame<'_>,
    model: &Model,
    active_job: Option<BackgroundJob>,
    spinner_tick: usize,
) {
    let filtered_indices = model.filtered_indices();
    let selected = model
        .selected_index(&filtered_indices)
        .and_then(|index| model.prs.get(index));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "PR Tracker",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("{} PRs", filtered_indices.len()),
            Style::default().fg(Color::White),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("view: {}", model.view_label()),
            Style::default().fg(Color::LightCyan),
        ),
        Span::raw("  |  "),
        Span::styled(
            match selected {
                Some(pr) => format!("Selected #{}/{}", pr.number, pr.repository),
                None => "Selected none".to_string(),
            },
            Style::default().fg(Color::Gray),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(header, chunks[0]);

    let items: Vec<ListItem<'_>> = filtered_indices
        .iter()
        .map(|pr_index| &model.prs[*pr_index])
        .enumerate()
        .map(|(index, pr)| {
            let ci_style = ci_style(pr.ci_status);
            let row_style = if index % 2 == 0 {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("#{:<6}", pr.number),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled(
                        format!("{} ", pr.author),
                        row_style.add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(truncate(&pr.title, 72), row_style),
                ]),
                Line::from(vec![
                    Span::styled("repo: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&pr.repository, Style::default().fg(Color::White)),
                    Span::raw("  "),
                    Span::styled("ci: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        ci_label(pr.ci_status),
                        ci_style.add_modifier(Modifier::BOLD),
                    ),
                    if pr.draft {
                        Span::styled("  draft", Style::default().fg(Color::Magenta))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(Span::styled(
                    pr.updates_since_last_ack(),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::raw(""),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(format!("{} Pull Requests", title_case(model.view_label())))
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(48, 56, 68))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !filtered_indices.is_empty() {
        state.select(Some(model.cursor));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);

    let spinner = match active_job {
        Some(job) => format!(
            "  |  {} {}",
            background_job_label(job),
            spinner_frame(spinner_tick)
        ),
        None => String::new(),
    };

    let footer = Paragraph::new(format!(
        "j/k or arrows: move  |  enter/space: open PR  |  a: acknowledge  |  v: toggle view  |  s: full sync  |  r: quick refresh  |  t: authors from teams  |  q: quit{}",
        spinner
    ))
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
}

fn draw_authors_screen(
    frame: &mut ratatui::Frame<'_>,
    model: &Model,
    active_job: Option<BackgroundJob>,
    spinner_tick: usize,
) {
    let state = &model.authors_screen;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(1),     // panes
            Constraint::Length(3),  // search bar
            Constraint::Length(2),  // footer
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "PR Tracker",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  |  "),
        Span::styled("Authors from Teams", Style::default().fg(Color::LightCyan)),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(header, chunks[0]);

    // Body
    if state.loading {
        let loading = Paragraph::new(format!(
            "Fetching team members... {}",
            spinner_frame(spinner_tick)
        ))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(loading, chunks[1]);
    } else if let Some(ref err) = state.error {
        let error = Paragraph::new(format!("Error fetching teams: {}", err))
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL).title("Error"));
        frame.render_widget(error, chunks[1]);
    } else {
        // Two-pane layout
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);

        // Left pane: Tracked
        let tracked_focused = state.focus == AuthorsPane::Tracked;
        let tracked_border_style = if tracked_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let tracked_filtered = state.filtered_list(&state.tracked);
        let tracked_items: Vec<ListItem<'_>> = tracked_filtered
            .iter()
            .map(|(_, login)| ListItem::new(login.as_str()))
            .collect();
        let tracked_list = List::new(tracked_items)
            .block(
                Block::default()
                    .title(if state.search_query.is_empty() || state.focus != AuthorsPane::Tracked {
                        format!("Tracked ({})", state.tracked.len())
                    } else {
                        format!("Tracked ({}/{})", tracked_filtered.len(), state.tracked.len())
                    })
                    .borders(Borders::ALL)
                    .border_style(tracked_border_style),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(48, 56, 68))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        let mut tracked_state = ListState::default();
        if !tracked_filtered.is_empty() && tracked_focused {
            tracked_state.select(Some(state.tracked_cursor.min(tracked_filtered.len() - 1)));
        }
        frame.render_stateful_widget(tracked_list, panes[0], &mut tracked_state);

        // Right pane: Untracked
        let untracked_focused = state.focus == AuthorsPane::Untracked;
        let untracked_border_style = if untracked_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let untracked_filtered = state.filtered_list(&state.untracked);
        let untracked_items: Vec<ListItem<'_>> = untracked_filtered
            .iter()
            .map(|(_, login)| ListItem::new(login.as_str()))
            .collect();
        let untracked_list = List::new(untracked_items)
            .block(
                Block::default()
                    .title(if state.search_query.is_empty() || state.focus != AuthorsPane::Untracked {
                        format!("Not Tracked ({})", state.untracked.len())
                    } else {
                        format!("Not Tracked ({}/{})", untracked_filtered.len(), state.untracked.len())
                    })
                    .borders(Borders::ALL)
                    .border_style(untracked_border_style),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(48, 56, 68))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        let mut untracked_state = ListState::default();
        if !untracked_filtered.is_empty() && untracked_focused {
            untracked_state.select(Some(state.untracked_cursor.min(untracked_filtered.len() - 1)));
        }
        frame.render_stateful_widget(untracked_list, panes[1], &mut untracked_state);
    }

    // Search bar — always render when data is loaded
    if !state.loading && state.error.is_none() {
        let search_text = if state.search_query.is_empty() {
            Line::from(Span::styled(
                "  / to search",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(vec![
                Span::styled("  /", Style::default().fg(Color::Yellow)),
                Span::raw(" "),
                Span::styled(&state.search_query, Style::default().fg(Color::White)),
                Span::styled("█", Style::default().fg(Color::Yellow)), // cursor
            ])
        };
        let search_bar = Paragraph::new(search_text)
            .block(Block::default().borders(Borders::ALL).title("Search"));
        frame.render_widget(search_bar, chunks[2]);
    } else {
        frame.render_widget(Block::default(), chunks[2]);
    }

    // Footer
    let spinner = match active_job {
        Some(BackgroundJob::TeamsFetch) => {
            format!("  |  fetching teams {}", spinner_frame(spinner_tick))
        }
        Some(job) => format!(
            "  |  {} {}",
            background_job_label(job),
            spinner_frame(spinner_tick)
        ),
        None => String::new(),
    };
    let footer = Paragraph::new(format!(
        "type to search  |  esc: clear/back  |  j/k: move  |  tab: switch pane  |  enter/space: track/untrack{}",
        spinner
    ))
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[3]);
}

fn spawn_full_sync(repo: DatabaseRepository, tx: mpsc::UnboundedSender<BackgroundMessage>) {
    tokio::spawn(async move {
        let progress_tx = tx.clone();
        let result = run_full_sync(repo, progress_tx).await;
        let _ = tx.send(BackgroundMessage::FullSyncFinished(result));
    });
}

fn spawn_quick_refresh(repo: DatabaseRepository, tx: mpsc::UnboundedSender<BackgroundMessage>) {
    tokio::spawn(async move {
        let progress_tx = tx.clone();
        let result = run_quick_refresh(repo, progress_tx).await;
        let _ = tx.send(BackgroundMessage::QuickRefreshFinished(result));
    });
}

fn spawn_teams_fetch(repo: DatabaseRepository, tx: mpsc::UnboundedSender<BackgroundMessage>) {
    tokio::spawn(async move {
        let result = run_teams_fetch(repo).await;
        let _ = tx.send(BackgroundMessage::TeamsFetchFinished(result));
    });
}

async fn run_full_sync(
    repo: DatabaseRepository,
    tx: mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<SyncRunSummary> {
    let user = repo.get_user().await?.ok_or_else(|| {
        anyhow::anyhow!("no authenticated user found, run 'cli auth <token>' first")
    })?;
    let github = GitHubClient::new(user.access_token)?;

    sync_all_tracked_with_progress(&repo, &github, |_| {
        let _ = tx.send(BackgroundMessage::Progress);
    })
    .await
}

async fn run_quick_refresh(
    repo: DatabaseRepository,
    tx: mpsc::UnboundedSender<BackgroundMessage>,
) -> anyhow::Result<QuickRefreshSummary> {
    let user = repo.get_user().await?.ok_or_else(|| {
        anyhow::anyhow!("no authenticated user found, run 'cli auth <token>' first")
    })?;
    let github = GitHubClient::new(user.access_token)?;

    refresh_existing_pull_requests_with_progress(&repo, &github, |_| {
        let _ = tx.send(BackgroundMessage::Progress);
    })
    .await
}

async fn run_teams_fetch(repo: DatabaseRepository) -> anyhow::Result<TeamsPayload> {
    let user = repo.get_user().await?.ok_or_else(|| {
        anyhow::anyhow!("no authenticated user found, run 'prt auth <token>' first")
    })?;
    let github = GitHubClient::new(user.access_token.clone())?;

    let tracked_authors = repo.get_tracked_authors().await?;
    let tracked_set: std::collections::HashSet<String> =
        tracked_authors.iter().map(|s| s.to_lowercase()).collect();
    let current_login_lower = user.username.to_lowercase();

    let teams = github.fetch_user_teams().await?;

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut all_members: Vec<String> = Vec::new();
    for team in &teams {
        let members = github
            .fetch_team_members(&team.organization.login, &team.slug)
            .await?;
        for member in members {
            let lower = member.login.to_lowercase();
            if lower != current_login_lower && seen.insert(lower.clone()) {
                all_members.push(member.login);
            }
        }
    }

    let mut untracked: Vec<String> = all_members
        .into_iter()
        .filter(|login| !tracked_set.contains(&login.to_lowercase()))
        .collect();
    untracked.sort();

    let mut tracked = tracked_authors;
    tracked.sort();

    Ok(TeamsPayload { tracked, untracked })
}

fn background_job_label(job: BackgroundJob) -> &'static str {
    match job {
        BackgroundJob::FullSync => "sync",
        BackgroundJob::QuickRefresh => "refresh",
        BackgroundJob::TeamsFetch => "fetching teams",
    }
}

fn spinner_frame(tick: usize) -> char {
    const FRAMES: [char; 4] = ['|', '/', '-', '\\'];
    FRAMES[tick % FRAMES.len()]
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
}

fn ci_style(status: CiStatus) -> Style {
    match status {
        CiStatus::Pending => Style::default().fg(Color::Yellow),
        CiStatus::Success => Style::default().fg(Color::Green),
        CiStatus::Failure => Style::default().fg(Color::Red),
    }
}

fn ci_label(status: CiStatus) -> &'static str {
    match status {
        CiStatus::Pending => "pending",
        CiStatus::Success => "success",
        CiStatus::Failure => "failure",
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}
