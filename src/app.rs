use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Layout},
    DefaultTerminal, Frame,
};

use crate::{
    action::Action,
    config::{Config, LocalSource, TidalConfig},
    players::{MusicPlayer, SafePlayer},
    sources::{
        local::LocalFiles,
        qobuz::QobuzSource,
        song::Song,
        streaming::{AuthStatus, StreamTrack, StreamingService},
        tidal::TidalSource,
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
    pending_auth: bool,
    deferred_search: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> =
            vec![LocalFiles::new("Local".to_string(), local_sources)];
        let (log_panel, logger) = LogPanel::new();
        logger.debug(format!("{something}", something = config));

        // Initialize streaming source based on config selection
        let streaming: Option<Box<dyn StreamingService>> = match config.streaming_service.as_str() {
            "tidal" => {
                let tidal_cfg = config.tidal.clone().unwrap_or_default();
                Some(Box::new(TidalSource::new(tidal_cfg)) as Box<dyn StreamingService>)
            }
            _ => {
                // Default: qobuz
                config.qobuz.as_ref().map(|q| {
                    Box::new(QobuzSource::with_credentials(
                        q.app_id.clone(),
                        q.app_secret.clone(),
                        q.email.clone(),
                        q.password.clone(),
                    )) as Box<dyn StreamingService>
                })
            }
        };

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
            pending_auth: false,
            deferred_search: None,
        }
    }

    /// Test constructor that accepts injected dependencies (no disk I/O, no network).
    pub fn new_for_test(
        config: Config,
        streaming: Option<Box<dyn StreamingService>>,
    ) -> Self {
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> =
            vec![LocalFiles::new("Local".to_string(), local_sources)];
        let (log_panel, logger) = LogPanel::new();

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
            pending_auth: false,
            deferred_search: None,
        }
    }

    /// Process one "tick" of the app loop: poll auth and handle pending searches.
    pub fn tick(&mut self) {
        self.poll_pending_auth();
        if let Some(query) = self.center_panel.take_pending_query() {
            self.perform_search(&query);
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.running = true;
        while self.running {
            // Poll player for updates
            if let Ok(info) = self.player.poll() {
                self.right_panel.update_playback_info(info);
            }

            // Poll pending auth (e.g. Tidal device code flow)
            self.poll_pending_auth();

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

    fn poll_pending_auth(&mut self) {
        if !self.pending_auth {
            return;
        }

        let service = match self.streaming.as_mut() {
            Some(s) => s,
            None => return,
        };

        match service.poll_auth() {
            Ok(true) => {
                self.pending_auth = false;
                self.logger
                    .info(format!("Authenticated with {}", service.name()));

                self.persist_streaming_credentials();

                // Retry deferred search
                if let Some(query) = self.deferred_search.take() {
                    self.perform_search(&query);
                }
            }
            Ok(false) => {
                // Still waiting
            }
            Err(e) => {
                self.pending_auth = false;
                self.deferred_search = None;
                self.logger
                    .error(format!("Auth polling failed: {}", e));
            }
        }
    }

    fn ensure_streaming_auth(&mut self) -> bool {
        // Already authenticated
        if let Some(ref service) = self.streaming {
            if service.is_authenticated() {
                return true;
            }
        }

        // Create source if needed based on config
        if self.streaming.is_none() {
            match self.config.streaming_service.as_str() {
                "tidal" => {
                    let tidal_cfg = self.config.tidal.clone().unwrap_or_default();
                    self.streaming =
                        Some(Box::new(TidalSource::new(tidal_cfg)));
                }
                _ => {
                    match &self.config.qobuz {
                        Some(q) if !q.email.is_empty() && !q.password.is_empty() => {
                            self.streaming = Some(Box::new(QobuzSource::with_credentials(
                                q.app_id.clone(),
                                q.app_secret.clone(),
                                q.email.clone(),
                                q.password.clone(),
                            )));
                        }
                        _ => {
                            self.logger.error(
                                "No streaming credentials configured. Set them in Settings > Account."
                                    .to_string(),
                            );
                            return false;
                        }
                    }
                }
            }
        }

        let service_name = self
            .streaming
            .as_ref()
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        self.logger
            .info(format!("Connecting to {}...", service_name));

        let service = self.streaming.as_mut().unwrap();
        match service.authenticate() {
            Ok(AuthStatus::Authenticated) => {
                self.logger
                    .info(format!("Authenticated with {}", service.name()));
                self.persist_streaming_credentials();
                true
            }
            Ok(AuthStatus::PendingUserAction(msg)) => {
                self.logger.info(msg);
                self.pending_auth = true;
                false
            }
            Err(e) => {
                self.logger.error(format!("Auth failed: {}", e));
                false
            }
        }
    }

    fn persist_streaming_credentials(&mut self) {
        let service = match self.streaming.as_ref() {
            Some(s) => s,
            None => return,
        };

        match service.name() {
            "Qobuz" => {
                if let Some((app_id, app_secret)) = service.app_credentials() {
                    if let Some(ref mut qobuz_config) = self.config.qobuz {
                        qobuz_config.app_id = app_id;
                        qobuz_config.app_secret = app_secret;
                        let _ = self.config.save();
                    }
                }
            }
            "Tidal" => {
                if let Some(data) = service.persist_data() {
                    if let Ok(tidal_cfg) = serde_json::from_str::<TidalConfig>(&data) {
                        self.config.tidal = Some(tidal_cfg);
                        let _ = self.config.save();
                    }
                }
            }
            _ => {}
        }
    }

    fn perform_search(&mut self, query: &str) {
        if !self.ensure_streaming_auth() {
            // If auth is pending (device flow), defer the search
            if self.pending_auth {
                self.deferred_search = Some(query.to_string());
            }
            return;
        }

        let service = self.streaming.as_mut().unwrap();
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

        self.logger
            .info(format!("Getting stream for {}...", track.display_title()));

        let service = match self.streaming.as_mut() {
            Some(s) => s,
            None => return,
        };

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

    pub fn render(&mut self, frame: &mut Frame) {
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
