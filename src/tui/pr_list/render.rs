use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph};

use crate::models::PullRequest;
use crate::tui::navigation::PrPane;
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
    let tracked_indices = state.tracked_indices(&shared.prs, &shared.username);
    let mine_indices = state.mine_indices(&shared.prs, &shared.username);
    let selected = state
        .selected_index_for_focus(&shared.prs, &shared.username)
        .and_then(|index| shared.prs.get(index));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let focus_label = match state.focus {
        PrPane::Tracked => "tracked",
        PrPane::Mine => "mine",
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "PR Tracker",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("tracked: {}", tracked_indices.len()),
            Style::default().fg(Color::White),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("mine: {}", mine_indices.len()),
            Style::default().fg(Color::White),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("view: {}", state.view_label()),
            Style::default().fg(Color::LightCyan),
        ),
        Span::raw("  |  "),
        Span::styled(
            format!("focus: {focus_label}"),
            Style::default().fg(Color::Gray),
        ),
        Span::raw("  |  "),
        Span::styled(
            match selected {
                Some(pr) => format!("Selected #{}/{}", pr.number, pr.repository),
                None => "Selected none".to_string(),
            },
            Style::default().fg(Color::Gray),
        ),
        match &shared.error {
            Some(error) => Span::styled(
                format!("  |  Error: {}", truncate(error, 60)),
                Style::default().fg(Color::Red),
            ),
            None => Span::raw(""),
        },
    ]))
    .block(Block::default().borders(Borders::ALL).title("Overview"));
    frame.render_widget(header, chunks[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    draw_pr_pane(
        frame,
        panes[0],
        "Tracked PRs",
        &tracked_indices,
        state.cursor_for(PrPane::Tracked),
        state.focus == PrPane::Tracked,
        shared,
    );
    draw_pr_pane(
        frame,
        panes[1],
        "My PRs",
        &mine_indices,
        state.cursor_for(PrPane::Mine),
        state.focus == PrPane::Mine,
        shared,
    );

    let spinner = match active_job {
        Some(job) => format!(
            "  |  {} {}",
            background_job_label(job),
            spinner_frame(spinner_tick)
        ),
        None => String::new(),
    };

    let footer = Paragraph::new(format!(
        "tab: switch pane  |  j/k or arrows: move  |  enter/space: open PR  |  ctrl+r: octo review  |  a: acknowledge  |  v: toggle view  |  s: full sync  |  t: authors from teams  |  q: quit{}",
        spinner
    ))
    .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
}

fn draw_pr_pane(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    title: &str,
    indices: &[usize],
    cursor: usize,
    focused: bool,
    shared: &SharedState,
) {
    let items: Vec<ListItem<'_>> = indices
        .iter()
        .map(|pr_index| &shared.prs[*pr_index])
        .enumerate()
        .map(|(index, pr)| build_list_item(index, pr, &shared.username))
        .collect();

    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(format!("{} ({})", title_case(title), indices.len()))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(48, 56, 68))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ")
        .highlight_spacing(HighlightSpacing::Always);

    let mut list_state = ListState::default();
    if focused && !indices.is_empty() {
        list_state.select(Some(cursor.min(indices.len() - 1)));
    }
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn build_list_item<'a>(index: usize, pr: &'a PullRequest, username: &str) -> ListItem<'a> {
    let ci_style = ci_style(pr.ci_status);
    let row_style = if index.is_multiple_of(2) {
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
            Span::styled(truncate(&pr.title, 48), row_style),
        ]),
        Line::from(vec![
            Span::styled("repo: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate(&pr.repository, 22),
                Style::default().fg(Color::White),
            ),
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
            approval_badge(pr),
            involved_badge(pr, username),
            review_badge(pr, username),
        ]),
        Line::from(Span::styled(
            pr.updates_since_last_ack(username),
            Style::default().fg(Color::DarkGray),
        )),
        Line::raw(""),
    ])
}
