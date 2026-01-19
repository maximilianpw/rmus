use ratatui::{
    layout::{Constraint, Layout, Rect},
    Frame,
};

use crate::{
    players::PlaybackInfo,
    ui::{log_panel::LogPanel, widget::now_playing_widget, AppPanel},
};

#[derive(Debug)]
pub struct RightPanel {
    pub log_panel: LogPanel,
    pub playback_info: PlaybackInfo,
}

const PLAYING_FILL: u16 = 3;

impl RightPanel {
    pub fn new(log_panel: LogPanel) -> Self {
        Self {
            log_panel,
            playback_info: PlaybackInfo::default(),
        }
    }

    pub fn update_playback_info(&mut self, info: PlaybackInfo) {
        self.playback_info = info;
    }
}

impl AppPanel for RightPanel {
    fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Fill(PLAYING_FILL), Constraint::Fill(1)]);
        let [playing_area, log_area] = layout.areas(area);

        self.log_panel.poll();

        now_playing_widget(&self.playback_info, is_focused, frame, playing_area);
        self.log_panel.render(frame, log_area, is_focused);
    }
}
