use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Tabs},
};

use crate::players::{PlaybackInfo, PlaybackState};

pub fn handle_focused_border_style(is_focused: bool) -> Style {
    if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    }
}

pub fn list_from_strings<'a>(list_items: &'a Vec<String>, is_focused: bool) -> List<'a> {
    let border_style = handle_focused_border_style(is_focused);
    let list_items: Vec<ListItem> = list_items
        .iter()
        .map(|i| ListItem::new(Line::from(i.as_str())))
        .collect();

    List::new(list_items)
        .block(
            Block::bordered()
                .title("Library")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
}

pub fn tabs_from_strings<'a>(
    tabs_items: &'a Vec<String>,
    selected_tab_index: usize,
    is_focused: bool,
) -> Tabs<'a> {
    let border_style = handle_focused_border_style(is_focused);
    Tabs::new(tabs_items.clone())
        .block(
            Block::bordered()
                .title("Sources")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .select(selected_tab_index)
        .highlight_style(Style::default().fg(Color::Yellow).bold())
}

pub fn now_playing_widget(info: &PlaybackInfo, is_focused: bool, frame: &mut Frame, area: Rect) {
    let border_style = handle_focused_border_style(is_focused);

    let title = match &info.current_song {
        Some(song) => song.title.clone(),
        None => "Not Playing".to_string(),
    };

    let block = Block::bordered()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Length(1), // Status
        Constraint::Length(1), // Progress
        Constraint::Length(1), // Time
    ])
    .split(inner);

    // Status line
    let status = match info.state {
        PlaybackState::Playing => Span::styled("Playing", Style::default().fg(Color::Green)),
        PlaybackState::Paused => Span::styled("Paused", Style::default().fg(Color::Yellow)),
        PlaybackState::Stopped => Span::styled("Stopped", Style::default().fg(Color::Gray)),
    };
    let volume = Span::raw(format!(" | Vol: {}%", info.volume));
    frame.render_widget(Paragraph::new(Line::from(vec![status, volume])), chunks[0]);

    // Progress bar
    let progress = if info.duration > 0.0 {
        (info.position / info.duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let gauge = Gauge::default()
        .ratio(progress)
        .gauge_style(Style::default().fg(Color::Cyan));
    frame.render_widget(gauge, chunks[1]);

    // Time display
    let pos_mins = (info.position / 60.0) as u32;
    let pos_secs = (info.position % 60.0) as u32;
    let dur_mins = (info.duration / 60.0) as u32;
    let dur_secs = (info.duration % 60.0) as u32;
    let time_str = format!(
        "{:02}:{:02} / {:02}:{:02}",
        pos_mins, pos_secs, dur_mins, dur_secs
    );
    frame.render_widget(
        Paragraph::new(time_str).alignment(Alignment::Center),
        chunks[2],
    );
}
