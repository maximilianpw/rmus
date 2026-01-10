use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
};

use crate::{
    config::{Config, LocalSource},
    players::{MusicPlayer, mpv::MpvPlayer},
    sources::{MusicSource, local::LocalFiles, song::Song},
    ui::{
        center_panel::CenterPanel,
        left_panel::LeftPanel,
        log_panel::{LogPanel, Logger},
        right_panel::RightPanel,
    },
};

use crate::event::handle_crossterm_events;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum FocusedWindow {
    #[default]
    Left,
    Center,
    Right,
    Logs,
}

impl FocusedWindow {
    pub fn next(&self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::Logs,
            Self::Logs => Self::Left,
        }
    }
}

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub focused_window: FocusedWindow,
    pub left_panel: LeftPanel,
    pub center_panel: CenterPanel,
    pub right_panel: RightPanel,
    pub player: MpvPlayer,
    pub logger: Logger,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> =
            vec![LocalFiles::new("Local".to_string(), local_sources)];
        let (log_panel, logger) = LogPanel::new();
        logger.info("Loaded config");
        logger.debug(format!("{something}", something = config));

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(),
            right_panel: RightPanel::new(log_panel),
            player: MpvPlayer::new(),
            logger,
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.running = true;
        while self.running {
            // Poll player for updates
            if let Ok(info) = self.player.poll() {
                self.right_panel.update_playback_info(info);
            }

            terminal.draw(|frame| self.render(frame))?;
            handle_crossterm_events(&mut self)?;
        }

        // Clean shutdown
        let _ = self.player.shutdown();
        Ok(())
    }

    pub fn play_album_from(&mut self, songs: Vec<Song>, index: usize) {
        if let Err(e) = self.player.play_album(songs, index) {
            self.logger.error(format!("Failed to play album: {e}"));
        }
    }

    pub fn toggle_pause(&mut self) {
        if let Err(e) = self.player.toggle_pause() {
            self.logger.error(format!("Failed to toggle pause: {e}"));
        }
    }

    pub fn next_track(&mut self) {
        if let Err(e) = self.player.next() {
            self.logger
                .error(format!("Failed to skip to next track: {e}"));
        }
    }

    pub fn previous_track(&mut self) {
        if let Err(e) = self.player.previous() {
            self.logger
                .error(format!("Failed to go to previous track: {e}"));
        }
    }

    pub fn stop_playback(&mut self) {
        if let Err(e) = self.player.stop() {
            self.logger.error(format!("Failed to stop playback: {e}"));
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Percentage(60),
            Constraint::Fill(1),
        ]);
        let [left_area, center_area, right_area] = layout.areas(frame.area());

        self.left_panel
            .render(frame, left_area, self.focused_window == FocusedWindow::Left);
        self.center_panel.render(
            frame,
            center_area,
            self.focused_window == FocusedWindow::Center,
        );
        self.right_panel.render(
            frame,
            right_area,
            self.focused_window == FocusedWindow::Right,
        );
    }

    pub(crate) fn quit(&mut self) {
        self.running = false;
    }
}
