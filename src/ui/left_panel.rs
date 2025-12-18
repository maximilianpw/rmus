use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Tabs, Widget},
};

use crate::ui::widget::list_from_strings;

#[derive(Debug, Default)]
pub struct LeftPanel {
    selected_tab_index: usize,
    tabs_items: Vec<&'static str>,
    list_items: Vec<String>,
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

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let layout = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]);
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

        let list = list_from_strings(&self.list_items, border_style);

        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => println!("adding a new folder"),
            KeyCode::Char('j') | KeyCode::Down => {
                let i = match self.list_state.selected() {
                    Some(i) => {
                        if i >= self.list_items.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.list_state.select(Some(i));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let i = match self.list_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.list_items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.list_state.select(Some(i));
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.selected_tab_index = (self.selected_tab_index + 1) % self.tabs_items.len();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.selected_tab_index = if self.selected_tab_index > 0 {
                    self.selected_tab_index - 1
                } else {
                    self.tabs_items.len() - 1
                };
            }
            _ => {}
        }
    }
}
