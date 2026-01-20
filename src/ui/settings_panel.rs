use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Constraint,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::ui::{widget::handle_focused_border_style, AppPanel};

#[derive(Debug)]
pub struct SettingsPanel {
    pub opened: bool,
    selected_tab_index: usize,
    tab_names: Vec<String>,
}

impl AppPanel for SettingsPanel {
    fn render(
        &mut self,
        frame: &mut ratatui::Frame,
        area: ratatui::prelude::Rect,
        is_focused: bool,
    ) {
        if !self.opened {
            return;
        }

        let area = area.centered(Constraint::Percentage(20), Constraint::Length(3));
        let border_style = handle_focused_border_style(is_focused);
        let popup = Paragraph::new("Popup content").block(
            Block::bordered()
                .title(self.tab_names[self.selected_tab_index].to_owned())
                .borders(Borders::ALL)
                .border_style(border_style),
        );
        frame.render_widget(Clear, area);
        frame.render_widget(popup, area);
    }
}

impl SettingsPanel {
    pub fn new() -> Self {
        SettingsPanel {
            opened: false,
            selected_tab_index: 0,
            tab_names: vec!["Settings".to_owned(), "Account".to_owned()],
        }
    }

    pub fn toggle_open(&mut self) {
        self.opened = !self.opened
    }
    pub fn open(&mut self) {
        self.opened = true
    }
    pub fn close(&mut self) {
        self.opened = false
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close(),
            KeyCode::Tab => self.toggle_open(),
            _ => {}
        }
    }

    pub fn next_tab(&mut self) {
        if !self.tab_names.is_empty() {
            self.selected_tab_index = (self.selected_tab_index + 1) % self.tab_names.len();
        }
    }
}
