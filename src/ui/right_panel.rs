use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
};

use crate::ui::{log_panel::LogPanel, widget::now_playing};

#[derive(Debug)]
pub struct RightPanel {
    pub log_panel: LogPanel,
}

const PLAYING_FILL: u16 = 3;
impl RightPanel {
    pub fn new(log_panel: LogPanel) -> Self {
        Self { log_panel }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Fill(PLAYING_FILL), Constraint::Fill(1)]);
        let [playing_area, log_area] = layout.areas(area);

        let now_playing_song = String::from("American Wedding");
        let now_playing = now_playing(&now_playing_song, is_focused);
        self.log_panel.poll();

        frame.render_widget(now_playing, playing_area);
        self.log_panel.render(frame, log_area, is_focused);
    }
}
