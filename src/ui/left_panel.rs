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

const TABS_HEIGHT: u16 = 3;

#[derive(Debug, Default)]
pub struct LeftPanel {
    selected_tab_index: usize,
    items: Vec<Box<dyn MusicSource>>,
    list_state: ListState,
    cached_items: Vec<String>,
}

impl LeftPanel {
    pub fn new(sources: Vec<Box<dyn MusicSource>>) -> Self {
        let mut panel = Self {
            selected_tab_index: 0,
            items: sources,
            list_state: ListState::default(),
            cached_items: Vec::new(),
        };
        panel.update_cache();
        panel
    }

    pub fn update_cache(&mut self) {
        if let Some(sources) = self.items.get(self.selected_tab_index) {
            self.cached_items = sources.get_albums();
        } else {
            self.cached_items.clear();
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Length(TABS_HEIGHT), Constraint::Fill(1)]);
        let [tabs_area, list_area] = layout.areas(area);
        let tab_names: Vec<String> = self.items.iter().map(|s| s.name()).collect();

        let tabs = tabs_from_strings(&tab_names, self.selected_tab_index, is_focused);
        frame.render_widget(tabs, tabs_area);

        let list = list_from_strings(&self.cached_items, is_focused);
        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            KeyCode::Char('l') | KeyCode::Right => self.next_tab(),
            KeyCode::Char('h') | KeyCode::Left => self.previous_tab(),
            _ => {}
        }
    }

    fn next_item(&mut self) {
        if self.cached_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.cached_items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous_item(&mut self) {
        if self.cached_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.cached_items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn next_tab(&mut self) {
        if !self.items.is_empty() {
            self.selected_tab_index = (self.selected_tab_index + 1) % self.items.len();
            self.list_state.select(Some(0));
            self.update_cache();
        }
    }

    fn previous_tab(&mut self) {
        if !self.items.is_empty() {
            self.selected_tab_index = if self.selected_tab_index > 0 {
                self.selected_tab_index - 1
            } else {
                self.items.len() - 1
            };
            self.list_state.select(Some(0));
            self.update_cache();
        }
    }
}
