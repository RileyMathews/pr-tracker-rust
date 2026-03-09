use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::models::PullRequest;
use crate::tui::pr_list::State;
use crate::tui::state::{title_case, truncate, SharedState};
use crate::tui::tasks::{background_job_label, BackgroundJob};
use crate::tui::widgets::{
    approval_badge, ci_label, ci_style, involved_badge, review_badge, spinner_frame,
};

/// Draw the PR List screen.
pub fn draw(
    frame: &mut ratatui::Frame<'_>,
    state: &State,
    shared: &SharedState,
    active_job: Option<BackgroundJob>,
    spinner_tick: usize,
) {
    let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
    let selected = state
        .selected_index(&filtered_indices)
        .and_then(|index| shared.prs.get(index));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "PR Tracker",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("{} PRs", filtered_indices.len()),
            Style::default().fg(Color::White),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("view: {}", state.view_label()),
            Style::default().fg(Color::LightCyan),
        ),
        Span::raw("  |  "),
        Span::styled(
            match selected {
                Some(pr) => format!("Selected #{}/{}", pr.number, pr.repository),
                None => "Selected none".to_string(),
            },
            Style::default().fg(Color::LightGray),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(header, chunks[0]);

    // PR List
    let items: Vec<ListItem<'_>> = filtered_indices
        .iter()
        .map(|pr_index| &shared.prs[*pr_index])
        .enumerate()
        .map(|(index, pr)| {
            let ci_style = ci_style(pr.ci_status);
            let row_style = if index % 2 == 0 {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::LightGray)
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("#{:<6}", pr.number),
                        Style::default().fg(Color::LightBlue),
                    ),
                    Span::styled(
                        format!("{} ", pr.author),
                        row_style.add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(truncate(&pr.title, 72), row_style),
                ]),
                Line::from(vec![
                    Span::styled("repo: ", Style::default().fg(Color::LightGray)),
                    Span::styled(&pr.repository, Style::default().fg(Color::White)),
                    Span::raw("  "),
                    Span::styled("ci: ", Style::default().fg(Color::LightGray)),
                    Span::styled(
                        ci_label(pr.ci_status),
                        ci_style.add_modifier(Modifier::BOLD),
                    ),
                    if pr.draft {
                        Span::styled("  draft", Style::default().fg(Color::LightMagenta))
                    } else {
                        Span::raw("")
                    },
                    approval_badge(pr),
                    involved_badge(pr, &shared.username),
                    review_badge(pr, &shared.username),
                ]),
                Line::from(Span::styled(
                    pr.updates_since_last_ack(),
                    Style::default().fg(Color::LightGray),
                )),
                Line::raw(""),
            ])
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(format!("{} Pull Requests", title_case(state.view_label())))
                .borders(Borders::ALL),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(80, 90, 110))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut list_state = ListState::default();
    if !filtered_indices.is_empty() {
        list_state.select(Some(state.cursor));
    }
    frame.render_stateful_widget(list, chunks[1], &mut list_state);

    // Footer with keybindings and spinner
    let spinner = match active_job {
        Some(job) => format!(
            "  |  {} {}",
            background_job_label(job),
            spinner_frame(spinner_tick)
        ),
        None => String::new(),
    };

    let footer = Paragraph::new(format!(
        "j/k or arrows: move  |  enter/space: open PR  |  a: acknowledge  |  v: toggle view  |  s: full sync  |  t: authors from teams  |  q: quit{}",
        spinner
    ))
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
}

/// Get the currently selected PR (if any) from state and shared data.
pub fn get_selected_pr<'a>(state: &State, shared: &'a SharedState) -> Option<&'a PullRequest> {
    let filtered_indices = state.filtered_indices(&shared.prs, &shared.username);
    state
        .selected_index(&filtered_indices)
        .and_then(|index| shared.prs.get(index))
}
