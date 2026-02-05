pub mod center_panel;
pub mod input_line;
pub mod left_panel;
pub mod log_panel;
pub mod right_panel;
pub mod settings;
mod widget;

use ratatui::{layout::Rect, Frame};

pub trait AppPanel {
    fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool);
}
