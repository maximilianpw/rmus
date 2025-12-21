use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

use crate::ui::widget::list_from_strings;

#[derive(Debug, Default)]
pub struct LogPanel {
    selected_folder: String,
}

impl LogPanel {
    pub fn new() -> Self {
        Self {
            selected_folder: String::new(),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let list = list_from_strings(&vec!["STIRNG".to_owned()], is_focused);
        frame.render_widget(area);
    }

    pub(crate) fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(' ') => println!("test"),
            KeyCode::Char('d') => println!("delete"),
            _ => {}
        }
    }
}
