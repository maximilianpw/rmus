use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Layout},
    DefaultTerminal, Frame,
};

use crate::{
    action::Action,
    config::{Config, LocalSource},
    players::{MusicPlayer, SafePlayer},
    sources::{
        local::LocalFiles,
        qobuz::QobuzSource,
        song::Song,
        streaming::{StreamTrack, StreamingService},
        MusicSource,
    },
    ui::{
        center_panel::CenterPanel, left_panel::LeftPanel, log_panel::{LogPanel, Logger},
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

pub struct App {
    pub running: bool,
    pub focused_window: FocusedWindow,
    pub left_panel: LeftPanel,
    pub center_panel: CenterPanel,
    pub right_panel: RightPanel,
    pub settings_panel: SettingsPanel,
    pub player: SafePlayer,
    streaming: Option<Box<dyn StreamingService>>,
    search_results: Vec<StreamTrack>,
    config: Config,
    logger: Logger,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> =
            vec![LocalFiles::new("Local".to_string(), local_sources)];
        let (log_panel, logger) = LogPanel::new();
        logger.debug(format!("{something}", something = config));

        // Initialize streaming source from config
        let streaming: Option<Box<dyn StreamingService>> = config.qobuz.as_ref().map(|q| {
            Box::new(QobuzSource::with_credentials(
                q.app_id.clone(),
                q.app_secret.clone(),
            )) as Box<dyn StreamingService>
        });

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(logger.clone()),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player: SafePlayer::new(),
            streaming,
            search_results: Vec::new(),
            config,
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

            // Check for pending search queries
            if let Some(query) = self.center_panel.take_pending_query() {
                self.perform_search(&query);
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
                if self.center_panel.is_showing_search_results() {
                    self.play_search_result();
                } else if let Some(index) = self.center_panel.get_selected_index() {
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
            Action::OpenSearch => {
                self.focused_window = FocusedWindow::Center;
                self.center_panel.open_search();
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

    fn ensure_streaming_auth(&mut self) -> bool {
        // Already authenticated
        if let Some(ref service) = self.streaming {
            if service.is_authenticated() {
                return true;
            }
        }

        // Get credentials from config
        let (email, password) = match &self.config.qobuz {
            Some(q) if !q.email.is_empty() && !q.password.is_empty() => {
                (q.email.clone(), q.password.clone())
            }
            _ => {
                self.logger
                    .error("No Qobuz credentials configured. Set them in Settings > Account.".to_string());
                return false;
            }
        };

        // Create source if needed
        if self.streaming.is_none() {
            self.streaming = Some(Box::new(QobuzSource::new()));
        }

        self.logger.info("Connecting to Qobuz...".to_string());

        let service = self.streaming.as_mut().unwrap();
        match service.authenticate(&email, &password) {
            Ok(()) => {
                self.logger
                    .info(format!("Authenticated with {}", service.name()));

                // Cache app credentials in config
                if let Some((app_id, app_secret)) = service.app_credentials() {
                    if let Some(ref mut qobuz_config) = self.config.qobuz {
                        qobuz_config.app_id = app_id;
                        qobuz_config.app_secret = app_secret;
                        let _ = self.config.save();
                    }
                }

                true
            }
            Err(e) => {
                self.logger.error(format!("Auth failed: {}", e));
                false
            }
        }
    }

    fn perform_search(&mut self, query: &str) {
        if !self.ensure_streaming_auth() {
            return;
        }

        let service = self.streaming.as_ref().unwrap();
        self.logger
            .info(format!("Searching {} for '{}'...", service.name(), query));

        match service.search(query, 20) {
            Ok(tracks) => {
                self.logger
                    .info(format!("Found {} results", tracks.len()));

                let songs: Vec<Song> = tracks
                    .iter()
                    .map(|t| Song {
                        title: t.display_title(),
                        path: std::path::PathBuf::new(),
                        url: None,
                    })
                    .collect();

                self.search_results = tracks;
                self.center_panel.set_search_results(songs);
            }
            Err(e) => {
                self.logger.error(format!("Search failed: {}", e));
            }
        }
    }

    fn play_search_result(&mut self) {
        let index = match self.center_panel.get_selected_index() {
            Some(i) => i,
            None => return,
        };

        let track = match self.search_results.get(index) {
            Some(t) => t.clone(),
            None => return,
        };

        let service = match self.streaming.as_ref() {
            Some(s) => s,
            None => return,
        };

        self.logger
            .info(format!("Getting stream for {}...", track.display_title()));

        match service.get_stream_url(&track.id) {
            Ok(Some(url)) => {
                let song = Song::from_url(track.display_title(), url);
                if let Err(e) = self.player.play(&song) {
                    self.logger.error(format!("Playback error: {}", e));
                }
            }
            Ok(None) => {
                self.logger
                    .error("Could not get stream URL for this track".to_string());
            }
            Err(e) => {
                self.logger
                    .error(format!("Stream URL error: {}", e));
            }
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

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("running", &self.running)
            .field("focused_window", &self.focused_window)
            .finish()
    }
}
