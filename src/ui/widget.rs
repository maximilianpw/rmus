use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem},
};

pub fn list_from_strings<'a>(list_items: &'a Vec<String>, border_style: Style) -> List<'a> {
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
