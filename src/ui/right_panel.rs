use ratatui::{
    layout::{Constraint, Layout, Rect},
    Frame,
};

use crate::{
    players::PlaybackInfo,
    ui::{
        log_panel::LogPanel,
        widget::{now_playing_progress_area, now_playing_widget},
        AppPanel,
    },
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

    pub(crate) fn layout_areas(area: Rect) -> (Rect, Rect) {
        let layout = Layout::vertical([Constraint::Fill(PLAYING_FILL), Constraint::Fill(1)]);
        let [playing_area, log_area] = layout.areas(area);

        (playing_area, log_area)
    }

    pub fn seek_position_at(&self, area: Rect, column: u16, row: u16) -> Option<f64> {
        let duration = self.playback_info.duration;
        if !duration.is_finite() || duration <= 0.0 {
            return None;
        }

        let progress_area = self.progress_area(area)?;
        if !rect_contains(progress_area, column, row) {
            return None;
        }

        let width = progress_area.width.saturating_sub(1);
        let ratio = if width == 0 {
            0.0
        } else {
            f64::from(column.saturating_sub(progress_area.x).min(width)) / f64::from(width)
        };
        Some(duration * ratio)
    }

    pub(crate) fn progress_area(&self, area: Rect) -> Option<Rect> {
        now_playing_progress_area(&self.playback_info, area)
    }

    pub fn render_with_focus(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        playback_focused: bool,
        logs_focused: bool,
    ) {
        let (playing_area, log_area) = Self::layout_areas(area);

        self.log_panel.poll();

        now_playing_widget(&self.playback_info, playback_focused, frame, playing_area);
        self.log_panel.render(frame, log_area, logs_focused);
    }
}

impl AppPanel for RightPanel {
    fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        self.render_with_focus(frame, area, is_focused, is_focused);
    }
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}
