use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
};

use crate::{
    sources::song::Song,
    ui::{log_panel::LogPanel, widget::now_playing},
};

#[derive(Debug)]
pub struct RightPanel {
    pub log_panel: LogPanel,
    pub selected_song: Option<Song>,
}

const PLAYING_FILL: u16 = 3;
impl RightPanel {
    pub fn new(log_panel: LogPanel) -> Self {
        Self {
            log_panel,
            selected_song: None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Fill(PLAYING_FILL), Constraint::Fill(1)]);
        let [playing_area, log_area] = layout.areas(area);

        let now_playing = now_playing(&self.selected_song, is_focused);
        self.log_panel.poll();

        frame.render_widget(now_playing, playing_area);
        self.log_panel.render(frame, log_area, is_focused);
    }

    pub fn play_song(&mut self, song: Song) {
        self.selected_song = Some(song);
    }
}
