use std::io;
use std::time::Duration;

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
}

impl Model {
    fn new(prs: Vec<PullRequest>) -> Self {
        Self { prs, cursor: 0 }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path =
        std::env::var("PR_TRACKER_DB").unwrap_or_else(|_| "sqlite://./db.sqlite3".to_string());
    let repo = DatabaseRepository::connect(&db_path).await?;
    repo.apply_migrations().await?;

    let prs = repo.get_all_prs().await?;
    run_tui(Model::new(prs))?;
    Ok(())
}

fn run_tui(mut model: Model) -> anyhow::Result<()> {
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
                        if model.cursor > 0 {
                            model.cursor -= 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if model.cursor + 1 < model.prs.len() {
                            model.cursor += 1;
                        }
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if let Some(pr) = model.prs.get(model.cursor) {
                            let _ = open::that(pr.url());
                        }
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let selected = model.prs.get(model.cursor);
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "PR Tracker",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("{} PRs", model.prs.len()),
            Style::default().fg(Color::White),
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

    let items: Vec<ListItem<'_>> = model
        .prs
        .iter()
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
                .title("Tracked Pull Requests")
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(48, 56, 68))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    if !model.prs.is_empty() {
        state.select(Some(model.cursor));
    }
    frame.render_stateful_widget(list, chunks[1], &mut state);

    let footer = Paragraph::new("j/k or arrows: move  |  enter/space: open PR  |  q: quit")
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
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
