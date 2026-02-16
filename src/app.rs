use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Layout},
    DefaultTerminal, Frame,
};

use crate::{
    action::Action,
    config::{Config, LocalSource},
    players::{mpv::MpvPlayer, MusicPlayer},
    sources::{local::LocalFiles, song::Song, MusicSource},
    ui::{
        center_panel::CenterPanel, left_panel::LeftPanel, log_panel::LogPanel,
        right_panel::RightPanel, settings::settings_panel::SettingsPanel, AppPanel,
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
    Settings,
}

impl FocusedWindow {
    pub fn next(&self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::Logs,
            _ => Self::Left,
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
    pub settings_panel: SettingsPanel,
    pub player: MpvPlayer,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> =
            vec![LocalFiles::new("Local".to_string(), local_sources)];
        let (log_panel, logger) = LogPanel::new();
        logger.debug(format!("{something}", something = config));

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(logger.clone()),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config),
            player: MpvPlayer::new(),
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

    pub fn execute(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit(),
            Action::SwitchPanel => self.focused_window = self.focused_window.next(),
            Action::ToggleSettings => self.settings_panel.toggle_open(),
            Action::SelectAlbum => {
                if let Some((path, songs)) = self.left_panel.get_selected_album() {
                    self.center_panel.set_album(path, songs);
                }
            }
            Action::PlaySelected => {
                if let Some(index) = self.center_panel.get_selected_index() {
                    let songs = self.center_panel.get_songs();
                    self.play_album_from(songs, index);
                }
            }
            Action::TogglePause => {
                let _ = self.player.toggle_pause();
            }
            Action::NextTrack => {
                let _ = self.player.next();
            }
            Action::PreviousTrack => {
                let _ = self.player.previous();
            }
            Action::StopPlayback => {
                let _ = self.player.stop();
            }
            Action::SeekForward(secs) => {
                let info = self.player.get_playback_info();
                let _ = self.player.seek(info.position + secs);
            }
            Action::SeekBackward(secs) => {
                let info = self.player.get_playback_info();
                let _ = self.player.seek((info.position - secs).max(0.0));
            }
            Action::VolumeUp(amount) => {
                let info = self.player.get_playback_info();
                let _ = self.player.set_volume(info.volume.saturating_add(amount));
            }
            Action::VolumeDown(amount) => {
                let info = self.player.get_playback_info();
                let _ = self.player.set_volume(info.volume.saturating_sub(amount));
            }
        }
    }

    pub fn delegate_key_to_panel(&mut self, key: KeyEvent) {
        if self.settings_panel.opened {
            self.settings_panel.handle_events(key);
            return;
        }
        match self.focused_window {
            FocusedWindow::Left => self.left_panel.handle_events(key),
            FocusedWindow::Center => self.center_panel.handle_events(key),
            FocusedWindow::Logs => self.right_panel.log_panel.handle_events(key),
            FocusedWindow::Settings => self.settings_panel.handle_events(key),
            _ => {}
        }
    }

    fn play_album_from(&mut self, songs: Vec<Song>, index: usize) {
        if let Err(e) = self.player.play_album(songs, index) {
            let _ = e;
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
        self.settings_panel.render(
            frame,
            frame.area(),
            self.focused_window == FocusedWindow::Settings,
        );
    }

    pub(crate) fn quit(&mut self) {
        self.running = false;
    }
}
