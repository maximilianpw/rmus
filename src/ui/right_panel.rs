use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

#[derive(Debug, Default)]
pub struct RightPanel {}

impl RightPanel {
    pub fn new() -> Self {
        Self {}
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        frame.render_widget(
            Block::bordered()
                .title("Right Column")
                .borders(Borders::ALL)
                .border_style(border_style),
            area,
        );
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(' ') => {
                println!("DEBUG: Center Panel: Play/Pause (Space)"); // For debugging
            }
            _ => {}
        }
    }
}
