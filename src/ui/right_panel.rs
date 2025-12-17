use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

pub struct RightPanel {}

impl RightPanel {
    pub fn new() -> Self {
        Self {}
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, is_focused: bool) {
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
}
