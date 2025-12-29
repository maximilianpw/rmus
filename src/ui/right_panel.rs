use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
};

use crate::ui::{
    log_panel::{LogPanel, Logger},
    widget::list_from_strings,
};

#[derive(Debug)]
pub struct RightPanel {
    pub log_panel: LogPanel,
    logger: Logger,
}

const PLAYING_FILL: u16 = 3;
impl RightPanel {
    pub fn new() -> Self {
        let (log_panel, logger) = LogPanel::new();
        Self { log_panel, logger }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let layout = Layout::vertical([Constraint::Fill(PLAYING_FILL), Constraint::Fill(1)]);
        let [playing_area, log_area] = layout.areas(area);

        let now_playing_list = vec!["LALA".to_owned()];
        let now_playing = list_from_strings(&now_playing_list, is_focused);
        self.log_panel.poll();

        frame.render_widget(now_playing, playing_area);
        self.log_panel.render(frame, log_area);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(' ') => self.logger.error("space pressed"),
            KeyCode::Char('d') => self.logger.debug("d pressed"),
            KeyCode::Char('i') => self.logger.info("i pressed"),
            _ => {}
        }
    }
}
