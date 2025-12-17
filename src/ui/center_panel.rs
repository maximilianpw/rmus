use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders},
};

pub fn render(frame: &mut Frame, area: Rect, is_focused: bool) {
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
