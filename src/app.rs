use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Layout},
    DefaultTerminal, Frame,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use crate::{
    action::Action,
    config::{Config, LocalSource, TidalConfig},
    players::{MusicPlayer, SafePlayer},
    playlist::{Playlist, PlaylistTrack},
    sources::{
        local::LocalFiles,
        qobuz::QobuzSource,
        song::Song,
        streaming::{
            AuthStatus, ResolvedStream, ResolvedStreamSource, StreamAlbum, StreamArtist,
            StreamTrack, StreamingService, StreamingServiceId,
        },
        tidal::TidalSource,
        MusicSource, PlaylistSource, StreamingTab,
    },
    ui::{
        center_panel::{CenterPanel, SearchMode},
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
    album_results: Vec<StreamAlbum>,
    artist_results: Vec<StreamArtist>,
    /// Which service produced the current search results (needed for playback).
    search_source: Option<StreamingServiceId>,
    config: Config,
    logger: Logger,
    /// Which service has a pending auth flow.
    pending_auth_service: Option<StreamingServiceId>,
    deferred_search: Option<String>,
    streaming_task_tx: Sender<StreamingTaskResult>,
    streaming_task_rx: Receiver<StreamingTaskResult>,
    busy_service: Option<StreamingServiceId>,
    next_task_id: u64,
    active_task: Option<ActiveStreamingTask>,
    canceled_task_ids: std::collections::HashSet<u64>,
    queued_search: Option<(StreamingServiceId, String)>,
    search_task_timeout: Duration,
    auth_task_timeout: Duration,
    stream_url_task_timeout: Duration,
    last_player_poll_error: Option<String>,
    last_player_runtime_error: Option<String>,
}

enum StreamingTask {
    SearchAlbums {
        query: String,
        limit: u32,
    },
    GetAlbumTracks {
        album_id: String,
        album_title: String,
    },
    PollAuth,
    GetStreamUrl {
        track_id: String,
        title: String,
        enqueue: bool,
    },
    /// Resolve stream URLs for an entire album. Resolves the start track first
    /// for immediate playback, then resolves the rest for enqueueing.
    PlayAlbumStream {
        tracks: Vec<StreamTrack>,
        start_index: usize,
    },
    SearchArtists {
        query: String,
        limit: u32,
    },
    SearchTracks {
        query: String,
        limit: u32,
    },
    GetArtistAlbums {
        artist_id: String,
        artist_name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamingTaskKind {
    Search,
    GetAlbumTracks,
    PollAuth,
    GetStreamUrl,
    PlayAlbumStream,
    SearchArtists,
    SearchTracks,
    GetArtistAlbums,
}

#[derive(Debug, Clone, Copy)]
struct ActiveStreamingTask {
    id: u64,
    service: StreamingServiceId,
    kind: StreamingTaskKind,
    started_at: Instant,
    timeout: Duration,
}

enum StreamingTaskOutput {
    AlbumSearchResults(Vec<StreamAlbum>),
    AlbumTracks {
        album_title: String,
        tracks: Vec<StreamTrack>,
    },
    AuthPending {
        message: String,
        deferred_query: Option<String>,
    },
    AuthCompleted,
    PollPending,
    StreamUrlResult {
        title: String,
        stream: Option<ResolvedStream>,
        enqueue: bool,
    },
    AlbumStreamUrls {
        /// The song at start_index, resolved and ready to play immediately.
        first_song: Option<Song>,
        /// All remaining songs resolved for enqueueing.
        remaining_songs: Vec<Song>,
        failed_count: usize,
    },
    ArtistSearchResults(Vec<StreamArtist>),
    TrackSearchResults(Vec<StreamTrack>),
    ArtistAlbums {
        artist_name: String,
        albums: Vec<StreamAlbum>,
    },
    Error(String),
}

struct StreamingTaskResult {
    task_id: u64,
    service_name: StreamingServiceId,
    service: Box<dyn StreamingService>,
    output: StreamingTaskOutput,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> = vec![
            LocalFiles::new("Local".to_string(), local_sources),
            Box::new(PlaylistSource::new()),
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
                config.audio.max_stream_quality,
            )) as Box<dyn StreamingService>
        });

        let tidal: Option<Box<dyn StreamingService>> = {
            let tidal_cfg = config.tidal.clone().unwrap_or_default();
            Some(
                Box::new(TidalSource::new(tidal_cfg, config.audio.max_stream_quality))
                    as Box<dyn StreamingService>,
            )
        };
        let (streaming_task_tx, streaming_task_rx) = mpsc::channel();

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player: SafePlayer::new(),
            qobuz,
            tidal,
            search_results: Vec::new(),
            album_results: Vec::new(),
            artist_results: Vec::new(),
            search_source: None,
            config,
            logger,
            pending_auth_service: None,
            deferred_search: None,
            streaming_task_tx,
            streaming_task_rx,
            busy_service: None,
            next_task_id: 1,
            active_task: None,
            canceled_task_ids: std::collections::HashSet::new(),
            queued_search: None,
            search_task_timeout: Duration::from_secs(20),
            auth_task_timeout: Duration::from_secs(10),
            stream_url_task_timeout: Duration::from_secs(20),
            last_player_poll_error: None,
            last_player_runtime_error: None,
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
            Box::new(PlaylistSource::new()),
            StreamingTab::boxed("Qobuz"),
            StreamingTab::boxed("Tidal"),
        ];
        let (log_panel, logger) = LogPanel::new();
        let (streaming_task_tx, streaming_task_rx) = mpsc::channel();

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player: SafePlayer::new(),
            qobuz,
            tidal,
            search_results: Vec::new(),
            album_results: Vec::new(),
            artist_results: Vec::new(),
            search_source: None,
            config,
            logger,
            pending_auth_service: None,
            deferred_search: None,
            streaming_task_tx,
            streaming_task_rx,
            busy_service: None,
            next_task_id: 1,
            active_task: None,
            canceled_task_ids: std::collections::HashSet::new(),
            queued_search: None,
            search_task_timeout: Duration::from_secs(20),
            auth_task_timeout: Duration::from_secs(10),
            stream_url_task_timeout: Duration::from_secs(20),
            last_player_poll_error: None,
            last_player_runtime_error: None,
        }
    }

    pub fn set_streaming_timeouts(
        &mut self,
        search_timeout: Duration,
        auth_timeout: Duration,
        stream_url_timeout: Duration,
    ) {
        self.search_task_timeout = search_timeout;
        self.auth_task_timeout = auth_timeout;
        self.stream_url_task_timeout = stream_url_timeout;
    }

    /// Process one "tick" of the app loop: sync config, poll auth, handle pending searches.
    pub fn tick(&mut self) {
        self.sync_config_from_settings();
        self.check_active_streaming_task_timeout();
        self.poll_streaming_task_results();
        self.poll_pending_auth();
        self.poll_streaming_task_results();
        if let Some(query) = self.center_panel.take_pending_query() {
            self.perform_search(&query);
            self.poll_streaming_task_results();
        }
        if let Some(index) = self.center_panel.take_pending_album_selection() {
            self.fetch_album_tracks(index);
            self.poll_streaming_task_results();
        }
        if let Some(index) = self.center_panel.take_pending_artist_selection() {
            self.fetch_artist_albums(index);
            self.poll_streaming_task_results();
        }
        if let Some(index) = self.center_panel.take_pending_queue_remove() {
            match self.player.remove_from_queue(index) {
                Ok(()) => {
                    self.logger.info("Removed from queue".to_string());
                    // Refresh queue view
                    let queue = self.player.get_queue().to_vec();
                    let pos = self.player.get_queue_position();
                    self.center_panel.set_queue(queue, pos);
                }
                Err(e) => self.logger.error(format!("Cannot remove: {}", e)),
            }
        }
        if let Some(index) = self.center_panel.take_pending_queue_jump() {
            let queue = self.player.get_queue().to_vec();
            if index < queue.len() {
                if let Err(e) = self.player.play_album(queue, index) {
                    self.logger.error(format!("Playback error: {}", e));
                }
            }
        }
        // Keep queue view in sync with player state
        if self.center_panel.is_showing_queue() {
            let queue = self.player.get_queue().to_vec();
            let pos = self.player.get_queue_position();
            self.center_panel.set_queue(queue, pos);
        }
        // Handle playlist creation
        if let Some(name) = self.center_panel.take_pending_playlist_create() {
            let playlist = Playlist::new(name.clone());
            match playlist.save() {
                Ok(()) => {
                    self.logger.info(format!("Created playlist '{}'", name));
                    self.rebuild_left_panel();
                }
                Err(e) => self
                    .logger
                    .error(format!("Failed to create playlist: {}", e)),
            }
        }
        // Handle add-to-playlist
        if let Some(index) = self.center_panel.take_pending_add_to_playlist() {
            let mut playlists = Playlist::load_all();
            if let Some(playlist) = playlists.get_mut(index) {
                let songs = self.center_panel.get_songs();
                for song in &songs {
                    let track = PlaylistTrack {
                        title: song.title.clone(),
                        artist: song.artist.clone(),
                        album_name: song.album_name.clone(),
                        path: if song.path.as_os_str().is_empty() {
                            None
                        } else {
                            Some(song.path.to_string_lossy().into_owned())
                        },
                        stream_service: None,
                        stream_track_id: None,
                    };
                    playlist.tracks.push(track);
                }
                match playlist.save() {
                    Ok(()) => {
                        self.logger.info(format!(
                            "Added {} song(s) to '{}'",
                            songs.len(),
                            playlist.name
                        ));
                        self.rebuild_left_panel();
                    }
                    Err(e) => self.logger.error(format!("Failed to save playlist: {}", e)),
                }
            }
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.running = true;
        while self.running {
            self.poll_player_state();

            // Sync config changes from settings panel
            self.sync_config_from_settings();

            // Process completed background tasks
            self.check_active_streaming_task_timeout();
            self.poll_streaming_task_results();

            // Poll pending auth (e.g. Tidal device code flow)
            self.poll_pending_auth();
            self.poll_streaming_task_results();

            // Check for pending search queries
            if let Some(query) = self.center_panel.take_pending_query() {
                self.perform_search(&query);
                self.poll_streaming_task_results();
            }

            // Check for pending album selections
            if let Some(index) = self.center_panel.take_pending_album_selection() {
                self.fetch_album_tracks(index);
                self.poll_streaming_task_results();
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
                if self.center_panel.is_showing_album_tracks() && self.search_source.is_some() {
                    // Album track selected — need stream URL
                    self.play_search_result();
                } else if self.center_panel.is_showing_search_results()
                    && self.search_source.is_some()
                {
                    // Legacy track search results — need stream URL
                    self.play_search_result();
                } else if let Some(index) = self.center_panel.get_selected_index() {
                    let songs = self.center_panel.get_songs();
                    self.play_album_from(songs, index);
                }
            }
            Action::TogglePause => {
                let result = self.player.toggle_pause();
                self.log_player_action_error("toggle pause", result);
            }
            Action::NextTrack => {
                let result = self.player.next();
                self.log_player_action_error("next track", result);
            }
            Action::PreviousTrack => {
                let result = self.player.previous();
                self.log_player_action_error("previous track", result);
            }
            Action::StopPlayback => {
                let result = self.player.stop();
                self.log_player_action_error("stop playback", result);
            }
            Action::SeekForward(secs) => {
                let info = self.player.get_playback_info();
                let result = self.player.seek(info.position + secs);
                self.log_player_action_error("seek forward", result);
            }
            Action::SeekBackward(secs) => {
                let info = self.player.get_playback_info();
                let result = self.player.seek((info.position - secs).max(0.0));
                self.log_player_action_error("seek backward", result);
            }
            Action::VolumeUp(amount) => {
                let info = self.player.get_playback_info();
                let result = self.player.set_volume(info.volume.saturating_add(amount));
                self.log_player_action_error("volume up", result);
            }
            Action::VolumeDown(amount) => {
                let info = self.player.get_playback_info();
                let result = self.player.set_volume(info.volume.saturating_sub(amount));
                self.log_player_action_error("volume down", result);
            }
            Action::ToggleShuffle => {
                let result = self.player.toggle_shuffle();
                self.log_player_action_error("toggle shuffle", result);
            }
            Action::CycleRepeat => {
                let result = self.player.cycle_repeat();
                self.log_player_action_error("cycle repeat", result);
            }
            Action::EnqueueSelected => {
                if self.center_panel.is_showing_album_tracks() && self.search_source.is_some() {
                    self.enqueue_search_result();
                } else if self.center_panel.is_showing_search_results()
                    && self.search_source.is_some()
                {
                    self.enqueue_search_result();
                } else {
                    let songs = self.center_panel.get_songs();
                    if !songs.is_empty() {
                        if let Err(e) = self.player.enqueue(songs) {
                            self.logger.error(format!("Enqueue error: {}", e));
                        } else {
                            self.logger.info("Added to queue".to_string());
                        }
                    }
                }
            }
            Action::ShowQueue => {
                let queue = self.player.get_queue().to_vec();
                let pos = self.player.get_queue_position();
                self.center_panel.set_queue(queue, pos);
                self.center_panel.show_queue();
                self.focused_window = FocusedWindow::Center;
            }
            Action::CreatePlaylist => {
                if self.left_panel.active_tab_name() == "Playlists" {
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_create_playlist();
                } else {
                    self.logger
                        .info("Switch to the Playlists tab first".to_string());
                }
            }
            Action::DeletePlaylist => {
                if self.left_panel.active_tab_name() == "Playlists" {
                    if let Some((path, _)) = self.left_panel.get_selected_album() {
                        let path_str = path.to_string_lossy();
                        if let Some(idx_str) = path_str.strip_prefix("playlist:") {
                            if let Ok(idx) = idx_str.parse::<usize>() {
                                let playlists = Playlist::load_all();
                                if let Some(playlist) = playlists.get(idx) {
                                    let name = playlist.name.clone();
                                    match Playlist::delete(&name) {
                                        Ok(()) => {
                                            self.logger
                                                .info(format!("Deleted playlist '{}'", name));
                                            self.rebuild_left_panel();
                                        }
                                        Err(e) => {
                                            self.logger.error(format!("Delete failed: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::AddToPlaylist => {
                let playlists = Playlist::load_all();
                if playlists.is_empty() {
                    self.logger.info(
                        "No playlists yet. Create one first (C on Playlists tab).".to_string(),
                    );
                } else {
                    let names: Vec<String> = playlists.iter().map(|p| p.name.clone()).collect();
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_playlist_picker(names);
                }
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

    fn take_service(
        &mut self,
        service_id: StreamingServiceId,
    ) -> Option<Box<dyn StreamingService>> {
        match service_id {
            StreamingServiceId::Qobuz => self.qobuz.take(),
            StreamingServiceId::Tidal => self.tidal.take(),
        }
    }

    fn put_service(&mut self, service_id: StreamingServiceId, service: Box<dyn StreamingService>) {
        match service_id {
            StreamingServiceId::Qobuz => self.qobuz = Some(service),
            StreamingServiceId::Tidal => self.tidal = Some(service),
        }
    }

    fn sync_config_from_settings(&mut self) {
        if let Some(mut new_config) = self.settings_panel.take_config_update() {
            // The settings UI does not own Tidal auth state. Preserve the live
            // credentials even if a stale settings copy emits a config update.
            new_config.tidal = self.config.tidal.clone();
            if let (Some(new_qobuz), Some(current_qobuz)) =
                (new_config.qobuz.as_mut(), self.config.qobuz.as_ref())
            {
                new_qobuz.app_id = current_qobuz.app_id.clone();
                new_qobuz.app_secret = current_qobuz.app_secret.clone();
            }

            let local_changed = self.config.local != new_config.local;
            let audio_changed = self.config.audio != new_config.audio;
            // Recreate Qobuz if credentials changed
            let qobuz_changed = self.config.qobuz != new_config.qobuz;
            let tidal_changed = self.config.tidal != new_config.tidal;
            if qobuz_changed || audio_changed {
                self.qobuz = new_config.qobuz.as_ref().map(|q| {
                    Box::new(QobuzSource::with_credentials(
                        q.app_id.clone(),
                        q.app_secret.clone(),
                        q.email.clone(),
                        q.password.clone(),
                        new_config.audio.max_stream_quality,
                    )) as Box<dyn StreamingService>
                });
            }
            if tidal_changed || audio_changed {
                let tidal_cfg = new_config.tidal.clone().unwrap_or_default();
                self.tidal = Some(Box::new(TidalSource::new(
                    tidal_cfg,
                    new_config.audio.max_stream_quality,
                )) as Box<dyn StreamingService>);
            }
            if local_changed {
                self.rebuild_left_panel();
            }

            self.config = new_config;
        }
    }

    fn rebuild_left_panel(&mut self) {
        let local_sources: Vec<LocalSource> = self.config.get_local_sources();
        let sources: Vec<Box<dyn MusicSource>> = vec![
            LocalFiles::new("Local".to_string(), local_sources),
            Box::new(PlaylistSource::new()),
            StreamingTab::boxed("Qobuz"),
            StreamingTab::boxed("Tidal"),
        ];
        self.left_panel = LeftPanel::new(sources, self.logger.clone());
    }

    fn poll_pending_auth(&mut self) {
        let service_id = match self.pending_auth_service {
            Some(id) => id,
            None => return,
        };

        if self.busy_service == Some(service_id) {
            return;
        }

        if let Some(service) = self.take_service(service_id) {
            self.spawn_streaming_task(service_id, service, StreamingTask::PollAuth);
        }
    }

    /// Check if a service's credentials have changed and persist them if so.
    /// Called after every background task completes to catch token refreshes.
    fn sync_service_credentials(&mut self, service_id: StreamingServiceId) {
        match service_id {
            StreamingServiceId::Tidal => {
                if let Some(ref service) = self.tidal {
                    if let Some(data) = service.persist_data() {
                        if let Ok(tidal_cfg) = serde_json::from_str::<TidalConfig>(&data) {
                            if self.config.tidal.as_ref() != Some(&tidal_cfg) {
                                self.config.tidal = Some(tidal_cfg);
                                if let Err(e) = self.config.save() {
                                    self.logger
                                        .error(format!("Failed to save refreshed token: {}", e));
                                }
                                self.settings_panel.update_config(&self.config);
                            }
                        }
                    }
                }
            }
            StreamingServiceId::Qobuz => {
                if let Some(ref service) = self.qobuz {
                    if let Some((app_id, app_secret)) = service.app_credentials() {
                        if let Some(ref qobuz_config) = self.config.qobuz {
                            if qobuz_config.app_id != app_id
                                || qobuz_config.app_secret != app_secret
                            {
                                if let Some(ref mut qc) = self.config.qobuz {
                                    qc.app_id = app_id;
                                    qc.app_secret = app_secret;
                                }
                                if let Err(e) = self.config.save() {
                                    self.logger
                                        .error(format!("Failed to save Qobuz credentials: {}", e));
                                }
                                self.settings_panel.update_config(&self.config);
                            }
                        }
                    }
                }
            }
        }
    }

    fn persist_streaming_credentials(&mut self, service_id: StreamingServiceId) {
        match service_id {
            StreamingServiceId::Qobuz => {
                if let Some(ref service) = self.qobuz {
                    if let Some((app_id, app_secret)) = service.app_credentials() {
                        if let Some(ref mut qobuz_config) = self.config.qobuz {
                            qobuz_config.app_id = app_id;
                            qobuz_config.app_secret = app_secret;
                        }
                    }
                }
            }
            StreamingServiceId::Tidal => {
                if let Some(ref service) = self.tidal {
                    if let Some(data) = service.persist_data() {
                        if let Ok(tidal_cfg) = serde_json::from_str::<TidalConfig>(&data) {
                            self.config.tidal = Some(tidal_cfg);
                        }
                    }
                }
            }
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
        let Some(service_id) = StreamingServiceId::from_tab_name(&tab) else {
            // Local filtering
            self.search_source = None;
            self.center_panel.filter_songs(query);
            return;
        };

        if self.busy_service == Some(service_id) {
            // Cancel only active searches so users can quickly replace queries.
            if let Some(active) = self.active_task {
                let is_search = matches!(
                    active.kind,
                    StreamingTaskKind::Search
                        | StreamingTaskKind::SearchArtists
                        | StreamingTaskKind::SearchTracks
                );
                if active.service == service_id && is_search {
                    self.cancel_active_streaming_task("Cancelling previous search...");
                    self.logger
                        .info("Replacing in-flight search...".to_string());
                } else {
                    self.logger
                        .info("Streaming request already in progress...".to_string());
                    return;
                }
            }
        }

        let search_mode = self.center_panel.search_mode();
        self.logger.info(format!(
            "Searching {} {} for '{}'...",
            service_id.as_str(),
            search_mode.label(),
            query
        ));

        let task = match search_mode {
            SearchMode::Albums => StreamingTask::SearchAlbums {
                query: query.to_string(),
                limit: 20,
            },
            SearchMode::Artists => StreamingTask::SearchArtists {
                query: query.to_string(),
                limit: 20,
            },
            SearchMode::Tracks => StreamingTask::SearchTracks {
                query: query.to_string(),
                limit: 20,
            },
        };

        if let Some(service) = self.take_service(service_id) {
            self.spawn_streaming_task(service_id, service, task);
        } else {
            self.queued_search = Some((service_id, query.to_string()));
            self.center_panel
                .set_status(Some("Waiting for previous request cleanup...".to_string()));
        }
    }

    fn fetch_album_tracks(&mut self, index: usize) {
        let album = match self.album_results.get(index) {
            Some(a) => a.clone(),
            None => return,
        };

        let service_id = match self.search_source {
            Some(id) => id,
            None => return,
        };

        if self.busy_service == Some(service_id) {
            self.logger
                .info("Streaming request already in progress...".to_string());
            return;
        }

        self.logger
            .info(format!("Loading tracks for '{}'...", album.title));

        if let Some(service) = self.take_service(service_id) {
            self.spawn_streaming_task(
                service_id,
                service,
                StreamingTask::GetAlbumTracks {
                    album_id: album.id,
                    album_title: format!("{} - {}", album.artist, album.title),
                },
            );
        }
    }

    fn fetch_artist_albums(&mut self, index: usize) {
        let artist = match self.artist_results.get(index) {
            Some(a) => a.clone(),
            None => return,
        };

        let service_id = match self.search_source {
            Some(id) => id,
            None => return,
        };

        if self.busy_service == Some(service_id) {
            self.logger
                .info("Streaming request already in progress...".to_string());
            return;
        }

        self.logger
            .info(format!("Loading albums for '{}'...", artist.name));

        if let Some(service) = self.take_service(service_id) {
            self.spawn_streaming_task(
                service_id,
                service,
                StreamingTask::GetArtistAlbums {
                    artist_id: artist.id,
                    artist_name: artist.name,
                },
            );
        }
    }

    fn play_search_result(&mut self) {
        let index = match self.center_panel.get_selected_index() {
            Some(i) => i,
            None => return,
        };

        // Use the service that produced these search results
        let service_id = match self.search_source {
            Some(id) => id,
            None => return,
        };

        if self.busy_service == Some(service_id) {
            self.logger
                .info("Streaming request already in progress...".to_string());
            return;
        }

        // If we're in album tracks mode and have multiple tracks, play the whole album
        if self.center_panel.is_showing_album_tracks() && self.search_results.len() > 1 {
            let tracks = self.search_results.clone();
            self.logger
                .info(format!("Resolving {} album tracks...", tracks.len()));
            if let Some(service) = self.take_service(service_id) {
                self.spawn_streaming_task(
                    service_id,
                    service,
                    StreamingTask::PlayAlbumStream {
                        tracks,
                        start_index: index,
                    },
                );
            }
            return;
        }

        let track = match self.search_results.get(index) {
            Some(t) => t.clone(),
            None => return,
        };

        self.logger
            .info(format!("Getting stream for {}...", track.display_title()));

        if let Some(service) = self.take_service(service_id) {
            let track_title = track.display_title();
            self.spawn_streaming_task(
                service_id,
                service,
                StreamingTask::GetStreamUrl {
                    track_id: track.id,
                    title: track_title,
                    enqueue: false,
                },
            );
        }
    }

    fn enqueue_search_result(&mut self) {
        let index = match self.center_panel.get_selected_index() {
            Some(i) => i,
            None => return,
        };

        let track = match self.search_results.get(index) {
            Some(t) => t.clone(),
            None => return,
        };

        self.logger.info(format!(
            "Getting stream to enqueue {}...",
            track.display_title()
        ));

        let service_id = match self.search_source {
            Some(id) => id,
            None => return,
        };

        if self.busy_service == Some(service_id) {
            self.logger
                .info("Streaming request already in progress...".to_string());
            return;
        }
        if let Some(service) = self.take_service(service_id) {
            self.spawn_streaming_task(
                service_id,
                service,
                StreamingTask::GetStreamUrl {
                    title: track.display_title(),
                    track_id: track.id,
                    enqueue: true,
                },
            );
        }
    }

    fn play_album_from(&mut self, songs: Vec<Song>, index: usize) {
        if let Err(e) = self.player.play_album(songs, index) {
            self.logger.error(format!("Playback error: {}", e));
        }
    }

    fn poll_player_state(&mut self) {
        match self.player.poll() {
            Ok(info) => {
                self.last_player_poll_error = None;
                if info.last_error != self.last_player_runtime_error {
                    if let Some(err) = &info.last_error {
                        self.logger.error(format!("Player error: {}", err));
                    }
                    self.last_player_runtime_error = info.last_error.clone();
                }
                self.right_panel.update_playback_info(info);
            }
            Err(e) => {
                let msg = e.to_string();
                if self.last_player_poll_error.as_deref() != Some(msg.as_str()) {
                    self.logger.error(format!("Player poll error: {}", msg));
                    self.last_player_poll_error = Some(msg);
                }
            }
        }
    }

    fn log_player_action_error(&mut self, action: &str, result: crate::players::PlayerResult<()>) {
        if let Err(e) = result {
            self.logger.error(format!("Could not {}: {}", action, e));
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

    fn spawn_streaming_task(
        &mut self,
        service_id: StreamingServiceId,
        mut service: Box<dyn StreamingService>,
        task: StreamingTask,
    ) {
        let (kind, timeout, status) = match &task {
            StreamingTask::SearchAlbums { .. } => (
                StreamingTaskKind::Search,
                self.search_task_timeout,
                format!("Searching {}...", service_id.as_str()),
            ),
            StreamingTask::GetAlbumTracks { .. } => (
                StreamingTaskKind::GetAlbumTracks,
                self.search_task_timeout,
                "Loading album tracks...".to_string(),
            ),
            StreamingTask::PollAuth => (
                StreamingTaskKind::PollAuth,
                self.auth_task_timeout,
                format!("Authenticating {}...", service_id.as_str()),
            ),
            StreamingTask::GetStreamUrl { .. } => (
                StreamingTaskKind::GetStreamUrl,
                self.stream_url_task_timeout,
                "Loading stream...".to_string(),
            ),
            StreamingTask::PlayAlbumStream { ref tracks, .. } => (
                StreamingTaskKind::PlayAlbumStream,
                // Allow more time for resolving multiple tracks
                Duration::from_secs(self.stream_url_task_timeout.as_secs() * tracks.len() as u64),
                "Resolving album streams...".to_string(),
            ),
            StreamingTask::SearchArtists { .. } => (
                StreamingTaskKind::SearchArtists,
                self.search_task_timeout,
                format!("Searching {} artists...", service_id.as_str()),
            ),
            StreamingTask::SearchTracks { .. } => (
                StreamingTaskKind::SearchTracks,
                self.search_task_timeout,
                format!("Searching {} tracks...", service_id.as_str()),
            ),
            StreamingTask::GetArtistAlbums { .. } => (
                StreamingTaskKind::GetArtistAlbums,
                self.search_task_timeout,
                "Loading artist albums...".to_string(),
            ),
        };
        let task_id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        self.center_panel.set_status(Some(status));
        self.busy_service = Some(service_id);
        self.active_task = Some(ActiveStreamingTask {
            id: task_id,
            service: service_id,
            kind,
            started_at: Instant::now(),
            timeout,
        });
        let tx = self.streaming_task_tx.clone();
        thread::spawn(move || {
            let output = match task {
                StreamingTask::SearchAlbums { query, limit } => {
                    if !service.is_authenticated() {
                        match service.authenticate() {
                            Ok(AuthStatus::Authenticated) => {
                                match service.search_albums(&query, limit) {
                                    Ok(albums) => StreamingTaskOutput::AlbumSearchResults(albums),
                                    Err(e) => {
                                        StreamingTaskOutput::Error(format!("Search failed: {}", e))
                                    }
                                }
                            }
                            Ok(AuthStatus::PendingUserAction(msg)) => {
                                StreamingTaskOutput::AuthPending {
                                    message: msg,
                                    deferred_query: Some(query),
                                }
                            }
                            Err(e) => StreamingTaskOutput::Error(format!("Auth failed: {}", e)),
                        }
                    } else {
                        match service.search_albums(&query, limit) {
                            Ok(albums) => StreamingTaskOutput::AlbumSearchResults(albums),
                            Err(e) => StreamingTaskOutput::Error(format!("Search failed: {}", e)),
                        }
                    }
                }
                StreamingTask::GetAlbumTracks {
                    album_id,
                    album_title,
                } => match service.get_album_tracks(&album_id) {
                    Ok(tracks) => StreamingTaskOutput::AlbumTracks {
                        album_title,
                        tracks,
                    },
                    Err(e) => StreamingTaskOutput::Error(format!("Failed to load album: {}", e)),
                },
                StreamingTask::PollAuth => match service.poll_auth() {
                    Ok(true) => StreamingTaskOutput::AuthCompleted,
                    Ok(false) => StreamingTaskOutput::PollPending,
                    Err(e) => StreamingTaskOutput::Error(format!("Auth polling failed: {}", e)),
                },
                StreamingTask::GetStreamUrl {
                    track_id,
                    title,
                    enqueue,
                } => match service.get_stream_url(&track_id) {
                    Ok(stream) => StreamingTaskOutput::StreamUrlResult {
                        title,
                        stream,
                        enqueue,
                    },
                    Err(e) => StreamingTaskOutput::Error(format!("Stream URL error: {}", e)),
                },
                StreamingTask::PlayAlbumStream {
                    tracks,
                    start_index,
                } => {
                    let mut failed_count = 0;
                    let mut resolved: Vec<Option<Song>> = Vec::with_capacity(tracks.len());

                    // Resolve the start track first for immediate playback
                    for (i, track) in tracks.iter().enumerate() {
                        if i == start_index {
                            match service.get_stream_url(&track.id) {
                                Ok(Some(stream)) => {
                                    let ResolvedStream {
                                        source,
                                        quality_label,
                                    } = stream;
                                    let song = match source {
                                        ResolvedStreamSource::Url(url) => Song::from_url(
                                            track.display_title(),
                                            url,
                                            quality_label,
                                        ),
                                        ResolvedStreamSource::Manifest {
                                            contents,
                                            file_extension,
                                        } => Song::from_manifest(
                                            track.display_title(),
                                            contents,
                                            file_extension,
                                            quality_label,
                                        ),
                                    };
                                    resolved.push(Some(song));
                                }
                                _ => {
                                    failed_count += 1;
                                    resolved.push(None);
                                }
                            }
                        } else {
                            resolved.push(None); // placeholder
                        }
                    }

                    // Resolve remaining tracks
                    for (i, track) in tracks.iter().enumerate() {
                        if i == start_index {
                            continue;
                        }
                        match service.get_stream_url(&track.id) {
                            Ok(Some(stream)) => {
                                let ResolvedStream {
                                    source,
                                    quality_label,
                                } = stream;
                                let song = match source {
                                    ResolvedStreamSource::Url(url) => {
                                        Song::from_url(track.display_title(), url, quality_label)
                                    }
                                    ResolvedStreamSource::Manifest {
                                        contents,
                                        file_extension,
                                    } => Song::from_manifest(
                                        track.display_title(),
                                        contents,
                                        file_extension,
                                        quality_label,
                                    ),
                                };
                                resolved[i] = Some(song);
                            }
                            _ => {
                                failed_count += 1;
                            }
                        }
                    }

                    let first_song = resolved[start_index].take();
                    // Build remaining songs in order, skipping start_index and failed ones
                    let mut remaining_songs = Vec::new();
                    // Add tracks after start_index first, then tracks before it
                    for i in (start_index + 1)..resolved.len() {
                        if let Some(song) = resolved[i].take() {
                            remaining_songs.push(song);
                        }
                    }
                    for i in 0..start_index {
                        if let Some(song) = resolved[i].take() {
                            remaining_songs.push(song);
                        }
                    }

                    StreamingTaskOutput::AlbumStreamUrls {
                        first_song,
                        remaining_songs,
                        failed_count,
                    }
                }
                StreamingTask::SearchArtists { query, limit } => {
                    if !service.is_authenticated() {
                        match service.authenticate() {
                            Ok(AuthStatus::Authenticated) => {
                                match service.search_artists(&query, limit) {
                                    Ok(artists) => {
                                        StreamingTaskOutput::ArtistSearchResults(artists)
                                    }
                                    Err(e) => StreamingTaskOutput::Error(format!(
                                        "Artist search failed: {}",
                                        e
                                    )),
                                }
                            }
                            Ok(AuthStatus::PendingUserAction(msg)) => {
                                StreamingTaskOutput::AuthPending {
                                    message: msg,
                                    deferred_query: Some(query),
                                }
                            }
                            Err(e) => StreamingTaskOutput::Error(format!("Auth failed: {}", e)),
                        }
                    } else {
                        match service.search_artists(&query, limit) {
                            Ok(artists) => StreamingTaskOutput::ArtistSearchResults(artists),
                            Err(e) => {
                                StreamingTaskOutput::Error(format!("Artist search failed: {}", e))
                            }
                        }
                    }
                }
                StreamingTask::SearchTracks { query, limit } => {
                    if !service.is_authenticated() {
                        match service.authenticate() {
                            Ok(AuthStatus::Authenticated) => match service.search(&query, limit) {
                                Ok(tracks) => StreamingTaskOutput::TrackSearchResults(tracks),
                                Err(e) => StreamingTaskOutput::Error(format!(
                                    "Track search failed: {}",
                                    e
                                )),
                            },
                            Ok(AuthStatus::PendingUserAction(msg)) => {
                                StreamingTaskOutput::AuthPending {
                                    message: msg,
                                    deferred_query: Some(query),
                                }
                            }
                            Err(e) => StreamingTaskOutput::Error(format!("Auth failed: {}", e)),
                        }
                    } else {
                        match service.search(&query, limit) {
                            Ok(tracks) => StreamingTaskOutput::TrackSearchResults(tracks),
                            Err(e) => {
                                StreamingTaskOutput::Error(format!("Track search failed: {}", e))
                            }
                        }
                    }
                }
                StreamingTask::GetArtistAlbums {
                    artist_id,
                    artist_name,
                } => match service.get_artist_albums(&artist_id) {
                    Ok(albums) => StreamingTaskOutput::ArtistAlbums {
                        artist_name,
                        albums,
                    },
                    Err(e) => {
                        StreamingTaskOutput::Error(format!("Failed to load artist albums: {}", e))
                    }
                },
            };

            let _ = tx.send(StreamingTaskResult {
                task_id,
                service_name: service_id,
                service,
                output,
            });
        });
    }

    fn poll_streaming_task_results(&mut self) {
        while let Ok(result) = self.streaming_task_rx.try_recv() {
            self.handle_streaming_task_result(result);
        }
        while self.busy_service.is_some() {
            match self
                .streaming_task_rx
                .recv_timeout(Duration::from_millis(20))
            {
                Ok(result) => self.handle_streaming_task_result(result),
                Err(_) => break,
            }
        }
    }

    fn handle_streaming_task_result(&mut self, result: StreamingTaskResult) {
        let was_active = self
            .active_task
            .map(|a| a.id == result.task_id)
            .unwrap_or(false);
        let is_canceled = self.canceled_task_ids.remove(&result.task_id);
        if was_active {
            self.active_task = None;
            self.busy_service = None;
            self.center_panel.set_status(None);
        }
        self.put_service(result.service_name, result.service);
        // Persist credentials if they changed (e.g. Tidal token refresh during a task)
        self.sync_service_credentials(result.service_name);
        if is_canceled {
            self.maybe_start_queued_search_for(result.service_name);
            return;
        }

        match result.output {
            StreamingTaskOutput::AlbumSearchResults(albums) => {
                self.logger.info(format!("Found {} albums", albums.len()));
                let display_titles: Vec<String> =
                    albums.iter().map(|a| a.display_title()).collect();
                self.album_results = albums;
                self.search_source = Some(result.service_name);
                self.center_panel.set_album_results(display_titles);
            }
            StreamingTaskOutput::AlbumTracks {
                album_title,
                tracks,
            } => {
                self.logger.info(format!("Loaded {} tracks", tracks.len()));
                let songs: Vec<Song> = tracks
                    .iter()
                    .map(|t| Song {
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        ..Default::default()
                    })
                    .collect();
                self.search_results = tracks;
                self.center_panel.set_album_tracks(album_title, songs);
            }
            StreamingTaskOutput::AuthPending {
                message,
                deferred_query,
            } => {
                self.pending_auth_service = Some(result.service_name);
                self.deferred_search = deferred_query;
                self.logger.info(message);
            }
            StreamingTaskOutput::AuthCompleted => {
                self.logger.info(format!(
                    "Authenticated with {}",
                    result.service_name.as_str()
                ));
                self.pending_auth_service = None;
                self.persist_streaming_credentials(result.service_name);
                if let Some(query) = self.deferred_search.take() {
                    self.perform_search(&query);
                }
            }
            StreamingTaskOutput::PollPending => {}
            StreamingTaskOutput::StreamUrlResult {
                title,
                stream,
                enqueue,
            } => match stream {
                Some(stream) => {
                    let ResolvedStream {
                        source,
                        quality_label,
                    } = stream;
                    let song = match source {
                        ResolvedStreamSource::Url(url) => Song::from_url(title, url, quality_label),
                        ResolvedStreamSource::Manifest {
                            contents,
                            file_extension,
                        } => Song::from_manifest(title, contents, file_extension, quality_label),
                    };
                    if enqueue {
                        if let Err(e) = self.player.enqueue(vec![song]) {
                            self.logger.error(format!("Enqueue error: {}", e));
                        } else {
                            self.logger.info("Added to queue".to_string());
                        }
                    } else if let Err(e) = self.player.play(&song) {
                        self.logger.error(format!("Playback error: {}", e));
                    }
                }
                None => {
                    self.logger
                        .error("Could not get stream URL for this track".to_string());
                }
            },
            StreamingTaskOutput::AlbumStreamUrls {
                first_song,
                remaining_songs,
                failed_count,
            } => {
                if let Some(song) = first_song {
                    if let Err(e) = self.player.play(&song) {
                        self.logger.error(format!("Playback error: {}", e));
                    } else if !remaining_songs.is_empty() {
                        if let Err(e) = self.player.enqueue(remaining_songs) {
                            self.logger.error(format!("Enqueue error: {}", e));
                        }
                    }
                } else {
                    self.logger
                        .error("Could not resolve the selected track".to_string());
                }
                if failed_count > 0 {
                    self.logger
                        .info(format!("{} track(s) could not be resolved", failed_count));
                }
            }
            StreamingTaskOutput::ArtistSearchResults(artists) => {
                self.logger.info(format!("Found {} artists", artists.len()));
                let display_titles: Vec<String> =
                    artists.iter().map(|a| a.display_title()).collect();
                self.artist_results = artists;
                self.search_source = Some(result.service_name);
                self.center_panel.set_artist_results(display_titles);
            }
            StreamingTaskOutput::TrackSearchResults(tracks) => {
                self.logger.info(format!("Found {} tracks", tracks.len()));
                let songs: Vec<Song> = tracks
                    .iter()
                    .map(|t| Song {
                        title: t.display_title(),
                        artist: t.artist.clone(),
                        ..Default::default()
                    })
                    .collect();
                self.search_results = tracks;
                self.search_source = Some(result.service_name);
                self.center_panel.set_search_results(songs);
            }
            StreamingTaskOutput::ArtistAlbums {
                artist_name,
                albums,
            } => {
                self.logger
                    .info(format!("Found {} albums for {}", albums.len(), artist_name));
                let display_titles: Vec<String> =
                    albums.iter().map(|a| a.display_title()).collect();
                self.album_results = albums;
                self.center_panel.set_album_results(display_titles);
            }
            StreamingTaskOutput::Error(msg) => {
                self.pending_auth_service = None;
                self.deferred_search = None;
                self.logger.error(msg);
            }
        }

        self.maybe_start_queued_search_for(result.service_name);
    }

    fn check_active_streaming_task_timeout(&mut self) {
        let Some(active) = self.active_task else {
            return;
        };
        if active.started_at.elapsed() <= active.timeout {
            return;
        }
        if self.canceled_task_ids.contains(&active.id) {
            return;
        }

        self.cancel_active_streaming_task(&format!(
            "{} request timed out; cleaning up...",
            active.service.as_str()
        ));
        self.logger.error(format!(
            "{} request timed out after {}s",
            active.service.as_str(),
            active.timeout.as_secs()
        ));
    }

    fn cancel_active_streaming_task(&mut self, status_message: &str) {
        let Some(active) = self.active_task else {
            return;
        };
        self.canceled_task_ids.insert(active.id);
        self.active_task = None;
        self.busy_service = None;
        self.center_panel
            .set_status(Some(status_message.to_string()));

        // Recreate the service immediately so a new request can start without
        // waiting for the canceled task thread to return.
        let _ = self.take_service(active.service);
        if let Some(service) = self.recreate_service(active.service) {
            self.put_service(active.service, service);
        }
    }

    fn recreate_service(
        &self,
        service_id: StreamingServiceId,
    ) -> Option<Box<dyn StreamingService>> {
        match service_id {
            StreamingServiceId::Qobuz => self.config.qobuz.as_ref().map(|q| {
                Box::new(QobuzSource::with_credentials(
                    q.app_id.clone(),
                    q.app_secret.clone(),
                    q.email.clone(),
                    q.password.clone(),
                    self.config.audio.max_stream_quality,
                )) as Box<dyn StreamingService>
            }),
            StreamingServiceId::Tidal => Some(Box::new(TidalSource::new(
                self.config.tidal.clone().unwrap_or_default(),
                self.config.audio.max_stream_quality,
            )) as Box<dyn StreamingService>),
        }
    }

    fn maybe_start_queued_search_for(&mut self, service_id: StreamingServiceId) {
        if self.busy_service.is_some() {
            return;
        }
        let queued = self.queued_search.take();
        match queued {
            Some((queued_service, query)) if queued_service == service_id => {
                self.perform_search(&query)
            }
            Some(other) => self.queued_search = Some(other),
            None => {}
        }
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::config::{AudioConfig, Config, LocalConfig, MaxStreamQuality, TidalConfig};

    use super::App;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn default_config() -> Config {
        Config {
            local: LocalConfig {
                sources: Vec::new(),
            },
            qobuz: None,
            tidal: None,
            audio: AudioConfig {
                default_volume: 50,
                max_stream_quality: MaxStreamQuality::HiRes,
            },
        }
    }

    #[test]
    fn stale_settings_update_does_not_clear_tidal_credentials() {
        let mut app = App::new_for_test(default_config(), None, None);
        app.config.tidal = Some(TidalConfig {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            country_code: "US".to_string(),
            token_expiry: 1_900_000_000,
        });

        app.settings_panel.toggle_open();
        app.settings_panel.handle_events(key(KeyCode::Char('q')));
        app.sync_config_from_settings();

        let tidal = app
            .config
            .tidal
            .as_ref()
            .expect("tidal credentials should be preserved");
        assert_eq!(tidal.access_token, "access");
        assert_eq!(tidal.refresh_token, "refresh");
        assert_eq!(tidal.country_code, "US");
        assert_eq!(tidal.token_expiry, 1_900_000_000);
    }
}
