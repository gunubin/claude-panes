use ansi_to_tui::IntoText;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use crate::app::App;
use crate::config::Config;
use crate::state::Status;

fn truncate_prompt(s: &str, max_width: usize) -> String {
    let first_line = s.lines().next().unwrap_or("");
    let mut width = 0;
    let mut result = String::new();
    for c in first_line.chars() {
        let w = c.width().unwrap_or(0);
        if width + w > max_width.saturating_sub(1) {
            result.push('…');
            return result;
        }
        width += w;
        result.push(c);
    }
    result
}

pub fn draw(f: &mut Frame, app: &App, config: &Config) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(config.list_percentage),
            Constraint::Percentage(config.preview_percentage),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Instance list (filtered)
    let items: Vec<ListItem> = app
        .filtered_indices
        .iter()
        .filter_map(|&idx| app.instances.get(idx).map(|inst| (idx, inst)))
        .map(|(idx, inst)| {
            let (icon, status_color) = match inst.status {
                Status::Working => ("●", Color::Green),
                Status::Waiting => ("◐", Color::Yellow),
                Status::Idle => ("○", Color::DarkGray),
                Status::Error => ("✕", Color::Red),
            };
            let status_label = match inst.status {
                Status::Working => "working",
                Status::Waiting => "waiting",
                Status::Idle => "idle",
                Status::Error => "error",
            };
            let keyword_tag = app
                .min_keywords
                .get(idx)
                .map(|k| format!(" [{}]", k))
                .unwrap_or_default();
            let mut spans = vec![
                Span::styled(format!("  {} ", icon), Style::default().fg(status_color)),
                Span::styled(inst.project.clone(), Style::default().fg(Color::White)),
                Span::styled(keyword_tag.clone(), Style::default().fg(Color::Cyan)),
                Span::raw(format!(
                    "{:>width$}",
                    "",
                    width = 28usize.saturating_sub(inst.project.len() + keyword_tag.len())
                )),
                Span::styled(
                    format!("{:<10}", status_label),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    format!("{:<10}", inst.position),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if !inst.last_prompt.is_empty() {
                let prompt = truncate_prompt(&inst.last_prompt, 40);
                spans.push(Span::styled(prompt, Style::default().fg(Color::DarkGray)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let title = if app.filter.is_empty() {
        " claude-panes ".to_string()
    } else {
        format!(" claude-panes > {} ", app.filter)
    };

    let border_style = Style::default().fg(config.border_color);

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        )
        .highlight_style(
            Style::default()
                .fg(config.border_color)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut list_state = ListState::default();
    list_state.select(Some(app.selected));
    f.render_stateful_widget(list, chunks[0], &mut list_state);

    // Preview pane
    let preview_title = app
        .selected_instance()
        .map(|i| format!(" Preview: {} ({}) ", i.project, i.position))
        .unwrap_or_else(|| " Preview ".to_string());

    let preview_text = app.preview.as_bytes().into_text().unwrap_or_default();

    let preview_height = chunks[1].height.saturating_sub(2).max(1) as usize;
    let total_lines = preview_text.lines.len();
    let scroll_offset = total_lines
        .saturating_sub(preview_height)
        .min(u16::MAX as usize) as u16;

    let preview = Paragraph::new(preview_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(preview_title),
        )
        .scroll((scroll_offset, 0));
    f.render_widget(preview, chunks[1]);

    // Help bar
    let key_style = Style::default().fg(config.border_color);
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" ↑↓", key_style),
        Span::raw(" navigate  "),
        Span::styled("Enter", key_style),
        Span::raw(" jump  "),
        Span::styled("Esc", key_style),
        Span::raw(" clear/quit"),
    ]));
    f.render_widget(help, chunks[2]);
}
