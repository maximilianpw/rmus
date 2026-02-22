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
        MusicSource, StreamingTab,
    },
    ui::{
        center_panel::CenterPanel,
        left_panel::LeftPanel,
        log_panel::{LogPanel, Logger},
        right_panel::RightPanel,
        settings::settings_panel::SettingsPanel,
        AppPanel,
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
    qobuz: Option<Box<dyn StreamingService>>,
    tidal: Option<Box<dyn StreamingService>>,
    search_results: Vec<StreamTrack>,
    /// Which service produced the current search results (needed for playback).
    search_source: Option<String>,
    config: Config,
    logger: Logger,
    /// Which service has a pending auth flow (e.g. "Tidal").
    pending_auth_service: Option<String>,
    deferred_search: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> = vec![
            LocalFiles::new("Local".to_string(), local_sources),
            StreamingTab::boxed("Qobuz"),
            StreamingTab::boxed("Tidal"),
        ];
        let (log_panel, logger) = LogPanel::new();
        logger.debug(format!("{something}", something = config));

        // Initialize both streaming services based on config
        let qobuz: Option<Box<dyn StreamingService>> = config.qobuz.as_ref().map(|q| {
            Box::new(QobuzSource::with_credentials(
                q.app_id.clone(),
                q.app_secret.clone(),
                q.email.clone(),
                q.password.clone(),
            )) as Box<dyn StreamingService>
        });

        let tidal: Option<Box<dyn StreamingService>> = {
            let tidal_cfg = config.tidal.clone().unwrap_or_default();
            Some(Box::new(TidalSource::new(tidal_cfg)) as Box<dyn StreamingService>)
        };

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(logger.clone()),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player: SafePlayer::new(),
            qobuz,
            tidal,
            search_results: Vec::new(),
            search_source: None,
            config,
            logger,
            pending_auth_service: None,
            deferred_search: None,
        }
    }

    /// Test constructor that accepts injected dependencies (no disk I/O, no network).
    pub fn new_for_test(
        config: Config,
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
    ) -> Self {
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> = vec![
            LocalFiles::new("Local".to_string(), local_sources),
            StreamingTab::boxed("Qobuz"),
            StreamingTab::boxed("Tidal"),
        ];
        let (log_panel, logger) = LogPanel::new();

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(logger.clone()),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player: SafePlayer::new(),
            qobuz,
            tidal,
            search_results: Vec::new(),
            search_source: None,
            config,
            logger,
            pending_auth_service: None,
            deferred_search: None,
        }
    }

    /// Process one "tick" of the app loop: sync config, poll auth, handle pending searches.
    pub fn tick(&mut self) {
        self.sync_config_from_settings();
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

            // Sync config changes from settings panel
            self.sync_config_from_settings();

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
                if self.center_panel.is_showing_search_results() && self.search_source.is_some() {
                    // Streaming search results — need stream URL
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
                let tab = self.left_panel.active_tab_name();
                if tab == "Qobuz" || tab == "Tidal" {
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_search();
                } else if !self.center_panel.get_songs().is_empty() {
                    // Local tab with songs loaded — open local filter search
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_search_local();
                } else {
                    self.logger
                        .info("No songs loaded. Select an album first.".to_string());
                }
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

    /// Look up the streaming service matching the given name.
    fn get_service(&self, name: &str) -> Option<&dyn StreamingService> {
        match name {
            "Qobuz" => self.qobuz.as_deref(),
            "Tidal" => self.tidal.as_deref(),
            _ => None,
        }
    }

    /// Look up the streaming service matching the given name (mutable).
    fn get_service_mut(&mut self, name: &str) -> Option<&mut Box<dyn StreamingService>> {
        match name {
            "Qobuz" => self.qobuz.as_mut(),
            "Tidal" => self.tidal.as_mut(),
            _ => None,
        }
    }

    fn sync_config_from_settings(&mut self) {
        if let Some(new_config) = self.settings_panel.take_config_update() {
            // Recreate Qobuz if credentials changed
            let qobuz_changed =
                format!("{:?}", self.config.qobuz) != format!("{:?}", new_config.qobuz);
            if qobuz_changed {
                self.qobuz = new_config.qobuz.as_ref().map(|q| {
                    Box::new(QobuzSource::with_credentials(
                        q.app_id.clone(),
                        q.app_secret.clone(),
                        q.email.clone(),
                        q.password.clone(),
                    )) as Box<dyn StreamingService>
                });
            }

            self.config = new_config;
        }
    }

    fn poll_pending_auth(&mut self) {
        let service_name = match &self.pending_auth_service {
            Some(name) => name.clone(),
            None => return,
        };

        let service = match self.get_service_mut(&service_name) {
            Some(s) => s,
            None => return,
        };

        match service.poll_auth() {
            Ok(true) => {
                self.logger
                    .info(format!("Authenticated with {}", service_name));
                self.pending_auth_service = None;

                self.persist_streaming_credentials(&service_name);

                // Retry deferred search
                if let Some(query) = self.deferred_search.take() {
                    self.perform_search(&query);
                }
            }
            Ok(false) => {
                // Still waiting
            }
            Err(e) => {
                self.pending_auth_service = None;
                self.deferred_search = None;
                self.logger.error(format!("Auth polling failed: {}", e));
            }
        }
    }

    /// Ensure the named streaming service is authenticated. Returns true if ready.
    fn ensure_streaming_auth(&mut self, service_name: &str) -> bool {
        // Already authenticated?
        if let Some(service) = self.get_service(service_name) {
            if service.is_authenticated() {
                return true;
            }
        }

        // Create source if needed based on config
        if self.get_service(service_name).is_none() {
            match service_name {
                "Tidal" => {
                    let tidal_cfg = self.config.tidal.clone().unwrap_or_default();
                    self.tidal = Some(Box::new(TidalSource::new(tidal_cfg)));
                }
                "Qobuz" => match &self.config.qobuz {
                    Some(q) if !q.email.is_empty() && !q.password.is_empty() => {
                        self.qobuz = Some(Box::new(QobuzSource::with_credentials(
                            q.app_id.clone(),
                            q.app_secret.clone(),
                            q.email.clone(),
                            q.password.clone(),
                        )));
                    }
                    _ => {
                        self.logger.error(
                            "No Qobuz credentials configured. Set them in Settings > Account."
                                .to_string(),
                        );
                        return false;
                    }
                },
                _ => return false,
            }
        }

        self.logger
            .info(format!("Connecting to {}...", service_name));

        let service = self.get_service_mut(service_name).unwrap();
        match service.authenticate() {
            Ok(AuthStatus::Authenticated) => {
                self.logger
                    .info(format!("Authenticated with {}", service_name));
                self.persist_streaming_credentials(service_name);
                true
            }
            Ok(AuthStatus::PendingUserAction(msg)) => {
                self.logger.info(msg);
                self.pending_auth_service = Some(service_name.to_string());
                false
            }
            Err(e) => {
                self.logger.error(format!("Auth failed: {}", e));
                false
            }
        }
    }

    fn persist_streaming_credentials(&mut self, service_name: &str) {
        match service_name {
            "Qobuz" => {
                if let Some(ref service) = self.qobuz {
                    if let Some((app_id, app_secret)) = service.app_credentials() {
                        if let Some(ref mut qobuz_config) = self.config.qobuz {
                            qobuz_config.app_id = app_id;
                            qobuz_config.app_secret = app_secret;
                        }
                    }
                }
            }
            "Tidal" => {
                if let Some(ref service) = self.tidal {
                    if let Some(data) = service.persist_data() {
                        if let Ok(tidal_cfg) = serde_json::from_str::<TidalConfig>(&data) {
                            self.config.tidal = Some(tidal_cfg);
                        }
                    }
                }
            }
            _ => {}
        }

        match self.config.save() {
            Ok(()) => self.logger.info("Credentials saved".to_string()),
            Err(e) => self.logger.error(format!("Failed to save config: {}", e)),
        }

        // Keep the settings panel's config copy in sync so it can't overwrite tokens
        self.settings_panel.update_config(&self.config);
    }

    fn perform_search(&mut self, query: &str) {
        // Determine search mode based on the active left panel tab
        let tab = self.left_panel.active_tab_name();
        if tab != "Qobuz" && tab != "Tidal" {
            // Local filtering
            self.search_source = None;
            self.center_panel.filter_songs(query);
            return;
        }

        let service_name = tab;

        if !self.ensure_streaming_auth(&service_name) {
            // If auth is pending (device flow), defer the search
            if self.pending_auth_service.is_some() {
                self.deferred_search = Some(query.to_string());
            }
            return;
        }

        self.logger
            .info(format!("Searching {} for '{}'...", service_name, query));

        let service = self.get_service_mut(&service_name).unwrap();
        match service.search(query, 20) {
            Ok(tracks) => {
                self.logger.info(format!("Found {} results", tracks.len()));

                let songs: Vec<Song> = tracks
                    .iter()
                    .map(|t| Song {
                        title: t.display_title(),
                        path: std::path::PathBuf::new(),
                        url: None,
                    })
                    .collect();

                self.search_results = tracks;
                self.search_source = Some(service_name);
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

        // Use the service that produced these search results
        let service_name = match &self.search_source {
            Some(name) => name.clone(),
            None => return,
        };

        let service = match self.get_service_mut(&service_name) {
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
                self.logger.error(format!("Stream URL error: {}", e));
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
