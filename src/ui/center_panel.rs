use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

#[derive(Debug, Default)]
pub struct CenterPanel {
    selected_folder: String,
}

impl CenterPanel {
    pub fn new() -> Self {
        Self {
            selected_folder: String::new(),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        frame.render_widget(
            Block::bordered()
                .title("Main Content")
                .borders(Borders::ALL)
                .border_style(border_style),
            area,
        );
    }

    pub(crate) fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(' ') => println!("test"),
            KeyCode::Char('d') => println!("delete"),
            _ => {}
        }
    }
}
