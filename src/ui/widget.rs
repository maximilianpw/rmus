use ratatui::{
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, Borders, List, ListItem, Tabs},
};

fn handle_focused_border_style(is_focused: bool) -> Style {
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

pub fn now_playing<'a>(selected_song: &'a String, is_focused: bool) -> Block<'a> {
    let border_style = handle_focused_border_style(is_focused);

    Block::bordered()
        .title(selected_song.to_owned())
        .borders(Borders::ALL)
        .border_style(border_style)
}
