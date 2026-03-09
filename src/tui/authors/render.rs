use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::tui::authors::State;
use crate::tui::navigation::AuthorsPane;
use crate::tui::tasks::{background_job_label, BackgroundJob};
use crate::tui::widgets::spinner_frame;

/// Draw the Authors screen.
pub fn draw(
    frame: &mut ratatui::Frame<'_>,
    state: &State,
    active_job: Option<BackgroundJob>,
    spinner_tick: usize,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(1),    // panes
            Constraint::Length(3), // search bar
            Constraint::Length(2), // footer
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
            .style(Style::default().fg(Color::LightRed))
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
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::LightGray)
        };
        let tracked_filtered = state.filtered_list(&state.tracked);
        let tracked_items: Vec<ListItem<'_>> = tracked_filtered
            .iter()
            .map(|(_, login)| ListItem::new(login.as_str()))
            .collect();
        let tracked_list = List::new(tracked_items)
            .block(
                Block::default()
                    .title(
                        if state.search_query.is_empty() || state.focus != AuthorsPane::Tracked {
                            format!("Tracked ({})", state.tracked.len())
                        } else {
                            format!(
                                "Tracked ({}/{})",
                                tracked_filtered.len(),
                                state.tracked.len()
                            )
                        },
                    )
                    .borders(Borders::ALL)
                    .border_style(tracked_border_style),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(80, 90, 110))
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
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::LightGray)
        };
        let untracked_filtered = state.filtered_list(&state.untracked);
        let untracked_items: Vec<ListItem<'_>> = untracked_filtered
            .iter()
            .map(|(_, login)| ListItem::new(login.as_str()))
            .collect();
        let untracked_list = List::new(untracked_items)
            .block(
                Block::default()
                    .title(
                        if state.search_query.is_empty() || state.focus != AuthorsPane::Untracked {
                            format!("Not Tracked ({})", state.untracked.len())
                        } else {
                            format!(
                                "Not Tracked ({}/{})",
                                untracked_filtered.len(),
                                state.untracked.len()
                            )
                        },
                    )
                    .borders(Borders::ALL)
                    .border_style(untracked_border_style),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(80, 90, 110))
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");
        let mut untracked_state = ListState::default();
        if !untracked_filtered.is_empty() && untracked_focused {
            untracked_state.select(Some(
                state.untracked_cursor.min(untracked_filtered.len() - 1),
            ));
        }
        frame.render_stateful_widget(untracked_list, panes[1], &mut untracked_state);
    }

    // Search bar — always render when data is loaded
    if !state.loading && state.error.is_none() {
        let search_text = if state.search_query.is_empty() {
            Line::from(Span::styled(
                "  / to search",
                Style::default().fg(Color::LightGray),
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
