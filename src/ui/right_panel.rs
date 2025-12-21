use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders},
};

use crate::ui::widget::list_from_strings;

#[derive(Debug, Default)]
pub struct RightPanel {}

const PLAYING_FILL: u16 = 3;
impl RightPanel {
    pub fn new() -> Self {
        Self {}
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Fill(PLAYING_FILL), Constraint::Fill(1)]);
        let [playing_area, log_area] = layout.areas(area);

        let now_playing = list_from_strings(&vec!["LALA".to_owned()], is_focused);

        frame.render_widget(now_playing, playing_area);

        let log_list = list_from_strings(&vec!["STIRNG".to_owned()], is_focused);
        frame.render_widget(log_list, log_area);
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
