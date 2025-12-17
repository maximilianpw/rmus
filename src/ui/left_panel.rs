use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Tabs},
};

#[derive(Debug, Default)]
pub struct LeftPanel {
    selected_tab_index: usize,
    tabs_items: Vec<&'static str>,
    list_items: Vec<String>, // Store actual list items
    list_state: ListState,
}

impl LeftPanel {
    pub fn new() -> Self {
        let tabs_items = vec!["Local", "Apple Music", "Cloud"];
        let list_items = vec![
            "Song 1 - Artist A".to_string(),
            "Song 2 - Artist B".to_string(),
            "Song 3 - Artist C".to_string(),
            "Song 4 - Artist D".to_string(),
        ];
        Self {
            selected_tab_index: 0,
            tabs_items,
            list_items,
            list_state: ListState::default(),
        }
    }

    // Render method will be added here
    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let layout = Layout::vertical([Constraint::Fill(1), Constraint::Percentage(90)]);
        let [tabs_area, list_area] = layout.areas(area);

        let tabs = Tabs::new(self.tabs_items.clone())
            .block(
                Block::bordered()
                    .title("Sources")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .select(self.selected_tab_index)
            .highlight_style(Style::default().fg(Color::Yellow).bold());
        frame.render_widget(tabs, tabs_area);

        let list_items: Vec<ListItem> = self
            .list_items
            .iter()
            .map(|i| ListItem::new(Line::from(i.as_str())))
            .collect();

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title("Library")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    pub fn handle_events(&self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') | KeyCode::Up => {}
            _ => {}
        }
    }
}
