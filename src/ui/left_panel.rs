use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::ListState,
};

use crate::{
    sources::MusicSource,
    ui::widget::{list_from_strings, tabs_from_strings},
};

#[derive(Debug, Default)]
pub struct LeftPanel {
    selected_tab_index: usize,
    items: Vec<Box<dyn MusicSource>>,
    list_state: ListState,
}

impl LeftPanel {
    pub fn new(sources: Vec<Box<dyn MusicSource>>) -> Self {
        Self {
            selected_tab_index: 0,
            items: sources,
            list_state: ListState::default(),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]);
        let [tabs_area, list_area] = layout.areas(area);
        let tab_names: Vec<String> = self.items.iter().map(|s| s.name()).collect();
        let list_items: Vec<String> = self.items[self.selected_tab_index].get_albums();

        let tabs = tabs_from_strings(&tab_names, self.selected_tab_index, is_focused);
        frame.render_widget(tabs, tabs_area);

        let list = list_from_strings(&list_items, is_focused);
        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('n') => println!("adding a new folder"),
            KeyCode::Char('j') | KeyCode::Down => {
                let list_items: Vec<String> = self.items[self.selected_tab_index].get_albums();
                if list_items.is_empty() {
                    return;
                }
                let i = match self.list_state.selected() {
                    Some(i) => {
                        if i >= list_items.len() - 1 {
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
                let list_items: Vec<String> = self.items[self.selected_tab_index].get_albums();
                if list_items.is_empty() {
                    return;
                }
                let i = match self.list_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            list_items.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.list_state.select(Some(i));
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.selected_tab_index = (self.selected_tab_index + 1) % self.items.len();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.selected_tab_index = if self.selected_tab_index > 0 {
                    self.selected_tab_index - 1
                } else {
                    self.items.len() - 1
                };
            }
            _ => {}
        }
    }
}
