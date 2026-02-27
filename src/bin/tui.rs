use std::io;
use std::time::Duration;

use chrono::Utc;
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use pr_tracker_rust::db::DatabaseRepository;
use pr_tracker_rust::models::CiStatus;
use pr_tracker_rust::models::PullRequest;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

struct Model {
    prs: Vec<PullRequest>,
    cursor: usize,
    view_mode: ViewMode,
}

#[derive(Clone, Copy)]
enum ViewMode {
    Active,
    Acknowledged,
}

impl Model {
    fn new(prs: Vec<PullRequest>) -> Self {
        Self {
            prs,
            cursor: 0,
            view_mode: ViewMode::Active,
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

                if include { Some(index) } else { None }
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path =
        std::env::var("PR_TRACKER_DB").unwrap_or_else(|_| "sqlite://./db.sqlite3".to_string());
    let repo = DatabaseRepository::connect(&db_path).await?;
    repo.apply_migrations().await?;

    let prs = repo.get_all_prs().await?;
    run_tui(Model::new(prs), &repo).await?;
    Ok(())
}

async fn run_tui(mut model: Model, repo: &DatabaseRepository) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut should_quit = false;
    while !should_quit {
        terminal.draw(|frame| draw(frame, &model))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
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
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn draw(frame: &mut ratatui::Frame<'_>, model: &Model) {
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

    let footer = Paragraph::new(
        "j/k or arrows: move  |  enter/space: open PR  |  a: acknowledge  |  v: toggle view  |  q: quit",
    )
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
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
