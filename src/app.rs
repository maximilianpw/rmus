use crossterm::event::KeyEvent;
use ratatui::{
    layout::{Constraint, Layout},
    DefaultTerminal, Frame,
};
use std::{
    collections::VecDeque,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    action::Action,
    config::{Config, LocalSource, TidalConfig},
    history::HistoryStore,
    players::{MusicPlayer, PlaybackState, SafePlayer},
    playlist::{PlaylistAddSummary, PlaylistRemoveSummary, PlaylistSource, PlaylistStore},
    queue::{QueueState, QueueStore},
    sources::{
        local::LocalFiles,
        qobuz::QobuzSource,
        song::Song,
        streaming::{
            ResolvedStream, ResolvedStreamSource, StreamAlbum, StreamArtist, StreamTrack,
            StreamingService, StreamingServiceId,
        },
        tidal::TidalSource,
        MusicSource, StreamingTab,
    },
    streaming_coordinator::{
        StreamingCoordinator, StreamingCoordinatorEvent, StreamingRequest, StreamingSubmitResult,
        StreamingTaskOutput,
    },
    ui::{
        center_panel::{CenterPanel, CenterPanelEvent, SearchMode},
        left_panel::LeftPanel,
        log_panel::{LogPanel, Logger},
        right_panel::RightPanel,
        settings::settings_panel::SettingsPanel,
        AppPanel,
    },
    utils::track_count_label,
};

use crate::event::handle_crossterm_events;

const FAVORITES_PLAYLIST_NAME: &str = "Favorites";
const PLAYBACK_HISTORY_LIMIT: usize = 50;

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
    pub player: Box<dyn MusicPlayer>,
    previous_focus_before_settings: Option<FocusedWindow>,
    streaming: StreamingCoordinator,
    playlist_store: PlaylistStore,
    history_store: HistoryStore,
    queue_store: QueueStore,
    pending_playlist_add_songs: Vec<Song>,
    pending_playlist_rename_index: Option<usize>,
    pending_playlist_duplicate_index: Option<usize>,
    pending_playlist_queue: VecDeque<Song>,
    pending_playlist_failed_count: usize,
    pending_playlist_active: bool,
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
    last_player_poll_error: Option<String>,
    last_player_runtime_error: Option<String>,
    muted_volume_before_zero: Option<u8>,
    playback_history: Vec<Song>,
    last_saved_queue_len: usize,
    last_saved_queue_position: usize,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let playlist_store = PlaylistStore::default();
        let history_store = HistoryStore::default();
        let queue_store = QueueStore::default();
        let restored_queue = queue_store.load();
        let sources = Self::sources_for_config(&config, playlist_store.clone(), false);
        let (log_panel, logger) = LogPanel::new();
        logger.debug(format!("{something}", something = config));

        // Initialize both streaming services based on config
        let qobuz: Option<Box<dyn StreamingService>> = config
            .qobuz
            .as_ref()
            .filter(|q| q.has_credentials())
            .map(|q| {
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
        let streaming = StreamingCoordinator::new(qobuz, tidal);
        let mut player: Box<dyn MusicPlayer> = Box::new(SafePlayer::new_with_playback_defaults(
            config.audio.default_volume,
            config.audio.default_shuffle,
            config.audio.default_repeat,
        ));
        Self::restore_player_queue(player.as_mut(), restored_queue, &logger);
        let (last_saved_queue_len, last_saved_queue_position) =
            Self::queue_save_marker(player.as_ref());

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player,
            previous_focus_before_settings: None,
            streaming,
            playlist_store,
            playback_history: history_store.load(),
            history_store,
            queue_store,
            pending_playlist_add_songs: Vec::new(),
            pending_playlist_rename_index: None,
            pending_playlist_duplicate_index: None,
            pending_playlist_queue: VecDeque::new(),
            pending_playlist_failed_count: 0,
            pending_playlist_active: false,
            search_results: Vec::new(),
            album_results: Vec::new(),
            artist_results: Vec::new(),
            search_source: None,
            config,
            logger,
            pending_auth_service: None,
            deferred_search: None,
            last_player_poll_error: None,
            last_player_runtime_error: None,
            muted_volume_before_zero: None,
            last_saved_queue_len,
            last_saved_queue_position,
        }
    }

    /// Test constructor that accepts injected dependencies (no disk I/O, no network).
    pub fn new_for_test(
        config: Config,
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
    ) -> Self {
        Self::new_for_test_with_playlist_store(config, qobuz, tidal, PlaylistStore::default())
    }

    pub fn new_for_test_with_playlist_store(
        config: Config,
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
        playlist_store: PlaylistStore,
    ) -> Self {
        let default_volume = config.audio.default_volume;
        let default_shuffle = config.audio.default_shuffle;
        let default_repeat = config.audio.default_repeat;
        Self::new_for_test_with_playlist_store_and_player(
            config,
            qobuz,
            tidal,
            playlist_store,
            Box::new(SafePlayer::new_with_playback_defaults(
                default_volume,
                default_shuffle,
                default_repeat,
            )),
        )
    }

    pub fn new_for_test_with_playlist_store_and_player(
        config: Config,
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
        playlist_store: PlaylistStore,
        player: Box<dyn MusicPlayer>,
    ) -> Self {
        Self::new_for_test_with_stores_and_player(
            config,
            qobuz,
            tidal,
            playlist_store,
            HistoryStore::with_path(Self::test_history_path()),
            player,
        )
    }

    fn test_history_path() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rmus-test-history-{}-{nanos}.toml",
            std::process::id()
        ))
    }

    fn test_queue_path() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rmus-test-queue-{}-{nanos}.toml",
            std::process::id()
        ))
    }

    pub fn new_for_test_with_stores_and_player(
        config: Config,
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
        playlist_store: PlaylistStore,
        history_store: HistoryStore,
        player: Box<dyn MusicPlayer>,
    ) -> Self {
        Self::new_for_test_with_all_stores_and_player(
            config,
            qobuz,
            tidal,
            playlist_store,
            history_store,
            QueueStore::with_path(Self::test_queue_path()),
            player,
        )
    }

    pub fn new_for_test_with_all_stores_and_player(
        config: Config,
        qobuz: Option<Box<dyn StreamingService>>,
        tidal: Option<Box<dyn StreamingService>>,
        playlist_store: PlaylistStore,
        history_store: HistoryStore,
        queue_store: QueueStore,
        mut player: Box<dyn MusicPlayer>,
    ) -> Self {
        let sources = Self::sources_for_config(&config, playlist_store.clone(), false);
        let (log_panel, logger) = LogPanel::new();
        let streaming = StreamingCoordinator::new(qobuz, tidal);
        let playback_history = history_store.load();
        let restored_queue = queue_store.load();
        Self::restore_player_queue(player.as_mut(), restored_queue, &logger);
        let (last_saved_queue_len, last_saved_queue_position) =
            Self::queue_save_marker(player.as_ref());

        Self {
            running: false,
            focused_window: FocusedWindow::default(),
            left_panel: LeftPanel::new(sources, logger.clone()),
            center_panel: CenterPanel::new(),
            right_panel: RightPanel::new(log_panel),
            settings_panel: SettingsPanel::new(config.clone()),
            player,
            previous_focus_before_settings: None,
            streaming,
            playlist_store,
            history_store,
            queue_store,
            pending_playlist_add_songs: Vec::new(),
            pending_playlist_rename_index: None,
            pending_playlist_duplicate_index: None,
            pending_playlist_queue: VecDeque::new(),
            pending_playlist_failed_count: 0,
            pending_playlist_active: false,
            search_results: Vec::new(),
            album_results: Vec::new(),
            artist_results: Vec::new(),
            search_source: None,
            config,
            logger,
            pending_auth_service: None,
            deferred_search: None,
            last_player_poll_error: None,
            last_player_runtime_error: None,
            muted_volume_before_zero: None,
            playback_history,
            last_saved_queue_len,
            last_saved_queue_position,
        }
    }

    fn sources_for_config(
        config: &Config,
        playlist_store: PlaylistStore,
        force_local_discovery: bool,
    ) -> Vec<Box<dyn MusicSource>> {
        let local_sources: Vec<LocalSource> = config.get_local_sources();
        let local: Box<dyn MusicSource> = if force_local_discovery {
            LocalFiles::new_fresh("Local".to_string(), local_sources)
        } else {
            LocalFiles::new("Local".to_string(), local_sources)
        };
        vec![
            local,
            Box::new(PlaylistSource::with_store(playlist_store)),
            StreamingTab::boxed("Qobuz"),
            StreamingTab::boxed("Tidal"),
        ]
    }

    pub fn set_streaming_timeouts(
        &mut self,
        search_timeout: Duration,
        auth_timeout: Duration,
        stream_url_timeout: Duration,
    ) {
        self.streaming
            .set_timeouts(search_timeout, auth_timeout, stream_url_timeout);
    }

    /// Process one "tick" of the app loop: sync config, poll auth, handle pending searches.
    pub fn tick(&mut self) {
        self.sync_config_from_settings();
        self.sync_qobuz_auth_request_from_settings();
        self.sync_tidal_auth_request_from_settings();
        self.check_active_streaming_task_timeout();
        self.poll_streaming_task_results();
        self.poll_pending_auth();
        self.poll_streaming_task_results();
        self.process_center_panel_events();
        self.sync_queue_view();
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.running = true;
        while self.running {
            self.poll_player_state();

            // Sync config changes from settings panel
            self.sync_config_from_settings();
            self.sync_qobuz_auth_request_from_settings();
            self.sync_tidal_auth_request_from_settings();

            // Process completed background tasks
            self.check_active_streaming_task_timeout();
            self.poll_streaming_task_results();

            // Poll pending auth (e.g. Tidal device code flow)
            self.poll_pending_auth();
            self.poll_streaming_task_results();

            self.process_center_panel_events();
            self.sync_queue_view();

            terminal.draw(|frame| self.render(frame))?;
            handle_crossterm_events(&mut self)?;
        }

        // Clean shutdown
        let _ = self.player.shutdown();
        Ok(())
    }

    fn process_center_panel_events(&mut self) {
        while let Some(event) = self.center_panel.next_event() {
            self.handle_center_panel_event(event);
        }
    }

    fn restore_player_queue(
        player: &mut dyn MusicPlayer,
        queue_state: QueueState,
        logger: &Logger,
    ) {
        if queue_state.tracks.is_empty() {
            return;
        }

        if let Err(e) = player.restore_queue(queue_state.tracks, queue_state.position) {
            logger.error(format!("Failed to restore queue: {}", e));
        }
    }

    fn queue_save_marker(player: &dyn MusicPlayer) -> (usize, usize) {
        let len = player.get_queue().len();
        let position = if len == 0 {
            0
        } else {
            player.get_queue_position().min(len - 1)
        };
        (len, position)
    }

    fn current_queue_state(&self) -> QueueState {
        QueueState::new(
            self.player.get_queue().to_vec(),
            self.player.get_queue_position(),
        )
    }

    fn save_queue_state(&mut self) {
        let state = self.current_queue_state();
        if let Err(e) = self.queue_store.save(&state) {
            self.logger.error(format!("Failed to save queue: {}", e));
        }
        self.last_saved_queue_len = state.tracks.len();
        self.last_saved_queue_position = state.position;
    }

    fn save_queue_state_if_marker_changed(&mut self) {
        let (len, position) = Self::queue_save_marker(self.player.as_ref());
        if len != self.last_saved_queue_len || position != self.last_saved_queue_position {
            self.save_queue_state();
        }
    }

    fn handle_center_panel_event(&mut self, event: CenterPanelEvent) {
        match event {
            CenterPanelEvent::QuerySubmitted(query) => {
                self.perform_search(&query);
                self.poll_streaming_task_results();
            }
            CenterPanelEvent::SongSelected => {
                self.execute(Action::PlaySelected);
                self.poll_streaming_task_results();
            }
            CenterPanelEvent::AlbumSelected(index) => {
                self.fetch_album_tracks(index);
                self.poll_streaming_task_results();
            }
            CenterPanelEvent::ArtistSelected(index) => {
                self.fetch_artist_albums(index);
                self.poll_streaming_task_results();
            }
            CenterPanelEvent::QueueItemRemoved(index) => {
                let removed_title = self
                    .player
                    .get_queue()
                    .get(index)
                    .map(Self::song_feedback_title);
                match self.player.remove_from_queue(index) {
                    Ok(()) => {
                        if let Some(title) = removed_title {
                            self.logger.info(format!("Removed {} from queue", title));
                        } else {
                            self.logger.info("Removed from queue".to_string());
                        }
                        self.save_queue_state();
                        let queue = self.player.get_queue().to_vec();
                        let pos = self.player.get_queue_position();
                        self.center_panel.set_queue(queue, pos);
                    }
                    Err(e) => self.logger.error(format!("Cannot remove: {}", e)),
                }
            }
            CenterPanelEvent::QueueCurrentItemRemovalBlocked => {
                let current_position = self.player.get_queue_position();
                let current_title = self
                    .player
                    .get_queue()
                    .get(current_position)
                    .map(Self::song_feedback_title);
                if let Some(title) = current_title {
                    self.logger.info(format!("Cannot remove: {}", title));
                } else {
                    self.logger
                        .info("Cannot remove currently playing track".to_string());
                }
            }
            CenterPanelEvent::QueueItemJumped(index) => {
                self.jump_to_queue_index(index);
            }
            CenterPanelEvent::QueueItemMoved { from, to } => {
                self.move_queue_item(from, to);
            }
            CenterPanelEvent::QueueClearRequested => {
                self.clear_queued_tracks();
            }
            CenterPanelEvent::QueueSaveRequested => {
                self.save_queue_to_playlist();
            }
            CenterPanelEvent::HistoryItemRemoved(index) => {
                self.remove_history_item(index);
            }
            CenterPanelEvent::HistoryClearRequested => {
                self.clear_history();
            }
            CenterPanelEvent::PlaylistCreated(name) => {
                match self.playlist_store.create(name.clone()) {
                    Ok(()) => {
                        self.logger.info(format!("Created playlist '{}'", name));
                        let songs = std::mem::take(&mut self.pending_playlist_add_songs);
                        if songs.is_empty() {
                            self.rebuild_left_panel();
                        } else {
                            self.add_songs_to_playlist_by_name(&name, &songs);
                        }
                        self.center_panel.complete_playlist_creation();
                    }
                    Err(e) => {
                        let message = if e.kind() == std::io::ErrorKind::AlreadyExists {
                            "Playlist already exists".to_string()
                        } else {
                            "Failed to create playlist".to_string()
                        };
                        self.logger
                            .error(format!("Failed to create playlist: {}", e));
                        self.center_panel.reject_playlist_creation(message);
                    }
                }
            }
            CenterPanelEvent::PlaylistRenamed(name) => {
                self.finish_playlist_rename(name);
            }
            CenterPanelEvent::PlaylistRenameCancelled => {
                self.pending_playlist_rename_index = None;
            }
            CenterPanelEvent::PlaylistDuplicated(name) => {
                self.finish_playlist_duplicate(name);
            }
            CenterPanelEvent::PlaylistDuplicateCancelled => {
                self.pending_playlist_duplicate_index = None;
            }
            CenterPanelEvent::PlaylistSelectedForAdd(index) => {
                let songs = std::mem::take(&mut self.pending_playlist_add_songs);
                if songs.is_empty() {
                    return;
                }
                self.add_songs_to_playlist_index(index, &songs);
            }
            CenterPanelEvent::PlaylistAddCancelled => self.pending_playlist_add_songs.clear(),
            CenterPanelEvent::PlaylistTrackRemoved { path, track_index } => {
                match self
                    .playlist_store
                    .remove_song_from_path(path.clone(), track_index)
                {
                    Ok(Some((playlist_name, removed_song, remaining_songs))) => {
                        self.logger.info(format!(
                            "Removed {} from '{}'",
                            Self::song_feedback_title(&removed_song),
                            playlist_name
                        ));
                        let title = format!(
                            "{} ({})",
                            playlist_name,
                            track_count_label(remaining_songs.len())
                        );
                        self.center_panel
                            .set_album_with_title(path, title, remaining_songs);
                        self.rebuild_left_panel();
                    }
                    Ok(None) => {
                        if PlaylistStore::is_playlist_path(&path) {
                            self.logger.info("Playlist no longer exists".to_string());
                            self.center_panel.clear_album_if_path(&path);
                            self.rebuild_left_panel();
                        } else {
                            self.logger
                                .info("Open a playlist to remove tracks".to_string());
                        }
                    }
                    Err(e) => self
                        .logger
                        .error(format!("Failed to update playlist: {}", e)),
                }
            }
            CenterPanelEvent::PlaylistTrackMoved { path, from, to } => {
                self.move_playlist_track(path, from, to);
            }
        }
    }

    fn open_rename_selected_playlist(&mut self) {
        let Some(index) = self.left_panel.selected_item_index() else {
            self.logger.info("No playlist selected".to_string());
            return;
        };
        let names = self.playlist_store.playlist_names();
        let Some(current_name) = names.get(index).cloned() else {
            self.logger.info("Playlist no longer exists".to_string());
            self.rebuild_left_panel();
            return;
        };

        self.pending_playlist_rename_index = Some(index);
        self.focused_window = FocusedWindow::Center;
        self.center_panel.open_rename_playlist(current_name);
    }

    fn finish_playlist_rename(&mut self, name: String) {
        let Some(index) = self.pending_playlist_rename_index else {
            self.logger.info("No playlist selected".to_string());
            self.center_panel.complete_playlist_rename();
            return;
        };

        match self.playlist_store.rename_at(index, name.clone()) {
            Ok(Some((old_name, new_name))) => {
                self.pending_playlist_rename_index = None;
                self.logger
                    .info(format!("Renamed playlist '{}' to '{}'", old_name, new_name));

                let old_path = PlaylistStore::path_for_name(&old_name);
                let new_path = PlaylistStore::path_for_name(&new_name);
                if self.center_panel.selected_album_path() == Some(&old_path) {
                    let songs = self.playlist_store.songs_for_name(&new_name);
                    let title = format!("{} ({})", new_name, track_count_label(songs.len()));
                    self.center_panel
                        .set_album_with_title(new_path, title, songs);
                }

                self.rebuild_left_panel();
                self.center_panel.complete_playlist_rename();
            }
            Ok(None) => {
                self.pending_playlist_rename_index = None;
                self.logger.info("Playlist no longer exists".to_string());
                self.rebuild_left_panel();
                self.center_panel.complete_playlist_rename();
            }
            Err(e) => {
                let message = match e.kind() {
                    std::io::ErrorKind::AlreadyExists => "Playlist already exists",
                    std::io::ErrorKind::InvalidInput => "Name required",
                    _ => "Failed to rename playlist",
                };
                self.logger
                    .error(format!("Failed to rename playlist: {}", e));
                self.center_panel
                    .reject_playlist_rename(message.to_string());
            }
        }
    }

    fn open_duplicate_selected_playlist(&mut self) {
        let Some(index) = self.left_panel.selected_item_index() else {
            self.logger.info("No playlist selected".to_string());
            return;
        };
        let names = self.playlist_store.playlist_names();
        let Some(current_name) = names.get(index).cloned() else {
            self.logger.info("Playlist no longer exists".to_string());
            self.rebuild_left_panel();
            return;
        };

        let suggested_name = Self::playlist_copy_name(&current_name, &names);
        self.pending_playlist_duplicate_index = Some(index);
        self.focused_window = FocusedWindow::Center;
        self.center_panel.open_duplicate_playlist(suggested_name);
    }

    fn finish_playlist_duplicate(&mut self, name: String) {
        let Some(index) = self.pending_playlist_duplicate_index else {
            self.logger.info("No playlist selected".to_string());
            self.center_panel.complete_playlist_duplicate();
            return;
        };

        match self.playlist_store.duplicate_at(index, name.clone()) {
            Ok(Some((old_name, new_name))) => {
                self.pending_playlist_duplicate_index = None;
                self.logger.info(format!(
                    "Duplicated playlist '{}' to '{}'",
                    old_name, new_name
                ));
                self.rebuild_left_panel();
                self.center_panel.complete_playlist_duplicate();
            }
            Ok(None) => {
                self.pending_playlist_duplicate_index = None;
                self.logger.info("Playlist no longer exists".to_string());
                self.rebuild_left_panel();
                self.center_panel.complete_playlist_duplicate();
            }
            Err(e) => {
                let message = match e.kind() {
                    std::io::ErrorKind::AlreadyExists => "Playlist already exists",
                    std::io::ErrorKind::InvalidInput => "Name required",
                    _ => "Failed to duplicate playlist",
                };
                self.logger
                    .error(format!("Failed to duplicate playlist: {}", e));
                self.center_panel
                    .reject_playlist_duplicate(message.to_string());
            }
        }
    }

    fn playlist_copy_name(current_name: &str, names: &[String]) -> String {
        let base = format!("{} Copy", current_name.trim());
        if !names.iter().any(|name| name.eq_ignore_ascii_case(&base)) {
            return base;
        }

        let mut copy_number = 2;
        loop {
            let candidate = format!("{base} {copy_number}");
            if !names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
            copy_number += 1;
        }
    }

    fn delete_selected_playlist(&mut self) {
        let Some(index) = self.left_panel.selected_item_index() else {
            self.logger.info("No playlist selected".to_string());
            return;
        };
        let selected_playlist_path = self.left_panel.get_selected_album().map(|(path, _)| path);

        match self.playlist_store.delete_at(index) {
            Ok(Some(name)) => {
                self.logger.info(format!("Deleted playlist '{}'", name));
                if let Some(path) = selected_playlist_path {
                    self.center_panel.clear_album_if_path(&path);
                }
                self.rebuild_left_panel();
            }
            Ok(None) => {
                self.logger.info("Playlist no longer exists".to_string());
                if let Some(path) = selected_playlist_path {
                    self.center_panel.clear_album_if_path(&path);
                }
                self.rebuild_left_panel();
            }
            Err(e) => {
                self.logger.error(format!("Delete failed: {}", e));
            }
        }
    }

    fn open_playlist_picker(&mut self, songs: Vec<Song>) {
        if songs.is_empty() {
            self.logger.info("Select a song first".to_string());
            return;
        }

        let names = self.playlist_store.playlist_names();
        if names.is_empty() {
            self.pending_playlist_add_songs = songs;
            self.focused_window = FocusedWindow::Center;
            self.center_panel.open_create_playlist();
        } else {
            self.pending_playlist_add_songs = songs;
            self.focused_window = FocusedWindow::Center;
            self.center_panel.open_playlist_picker(names);
        }
    }

    fn add_songs_to_playlist_by_name(&mut self, playlist_name: &str, songs: &[Song]) {
        let index = self
            .playlist_store
            .playlist_names()
            .iter()
            .position(|name| name == playlist_name);

        if let Some(index) = index {
            self.add_songs_to_playlist_index(index, songs);
        } else {
            self.logger.info("Playlist no longer exists".to_string());
            self.rebuild_left_panel();
        }
    }

    fn add_songs_to_playlist_index(&mut self, index: usize, songs: &[Song]) {
        match self.playlist_store.add_songs_to_index(index, songs) {
            Ok(Some((playlist_name, song_count))) => {
                self.logger.info(Self::playlist_add_feedback(
                    &playlist_name,
                    songs,
                    song_count,
                ));
            }
            Ok(None) => self.logger.info("Playlist no longer exists".to_string()),
            Err(e) => self.logger.error(format!("Failed to save playlist: {}", e)),
        }
        self.rebuild_left_panel();
    }

    fn log_empty_left_selection(&self) {
        let tab = self.left_panel.active_tab_name();
        let message = match tab.as_str() {
            "Local" => "No local albums. Add a source in Settings.",
            "Playlists" => "No playlists yet. Create one first (C on Playlists tab).",
            "Qobuz" | "Tidal" => {
                self.logger.info(format!("Use / to search {}.", tab));
                return;
            }
            _ => "No album selected.",
        };
        self.logger.info(message.to_string());
    }

    fn play_selected_collection(&mut self) {
        let Some((path, songs)) = self.left_panel.get_selected_album() else {
            self.log_empty_left_selection();
            return;
        };

        if songs.is_empty() {
            self.logger.info("No songs to play".to_string());
            return;
        }

        if let Some(title) = self.left_panel.selected_item_label() {
            self.center_panel
                .set_album_with_title(path, title, songs.clone());
        } else {
            self.center_panel.set_album(path, songs.clone());
        }
        self.play_album_from(songs, 0);
    }

    fn enqueue_selected_collection(&mut self) {
        let Some((_path, songs)) = self.left_panel.get_selected_album() else {
            self.log_empty_left_selection();
            return;
        };

        self.enqueue_collection(songs);
    }

    fn enqueue_collection(&mut self, songs: Vec<Song>) {
        if songs.is_empty() {
            self.logger.info("No songs to queue".to_string());
            return;
        }

        if songs.iter().any(Song::has_stream_reference) {
            self.logger
                .info(format!("Queueing {}...", track_count_label(songs.len())));
            self.pending_playlist_queue = songs.into();
            self.pending_playlist_failed_count = 0;
            self.pending_playlist_active = true;
            self.enqueue_next_pending_playlist_track();
            return;
        }

        let feedback = Self::queue_feedback(&songs);
        if let Err(e) = self.player.enqueue(songs) {
            self.logger.error(format!("Enqueue error: {}", e));
        } else {
            self.save_queue_state();
            self.logger.info(feedback);
        }
    }

    fn enqueue_open_collection(&mut self) {
        if !self.center_panel.is_showing_queueable_collection() {
            self.logger
                .info("Open an album or playlist first".to_string());
            return;
        }

        self.enqueue_collection(self.center_panel.get_songs());
    }

    fn add_open_collection_to_playlist(&mut self) {
        if !self.center_panel.is_showing_queueable_collection() {
            self.logger
                .info("Open an album or playlist first".to_string());
            return;
        }

        let songs = self.center_panel.get_songs();
        if songs.is_empty() {
            if self.center_panel.selected_album_path().is_some() {
                self.logger.info("No songs to add".to_string());
            } else {
                self.logger
                    .info("Open an album or playlist first".to_string());
            }
            return;
        }

        self.open_playlist_picker(songs);
    }

    fn add_current_track_to_playlist(&mut self) {
        let Some(song) = self.player.get_playback_info().current_song.clone() else {
            self.logger.info("No current track".to_string());
            return;
        };

        self.open_playlist_picker(vec![song]);
    }

    fn favorite_target_songs(&self, empty_left_message: &str) -> Option<Vec<Song>> {
        match self.focused_window {
            FocusedWindow::Left => {
                let Some((_path, songs)) = self.left_panel.get_selected_album() else {
                    self.log_empty_left_selection();
                    return None;
                };
                if songs.is_empty() {
                    self.logger.info(empty_left_message.to_string());
                    return None;
                }
                Some(songs)
            }
            FocusedWindow::Right => match self.player.get_playback_info().current_song.clone() {
                Some(song) => Some(vec![song]),
                None => {
                    self.logger.info("No current track".to_string());
                    None
                }
            },
            _ => {
                let songs = self.center_panel.selected_songs_for_playlist();
                if songs.is_empty() {
                    self.logger.info("Select a song first".to_string());
                    return None;
                }
                Some(songs)
            }
        }
    }

    fn add_to_favorites(&mut self) {
        let Some(songs) = self.favorite_target_songs("No songs to favorite") else {
            return;
        };

        match self
            .playlist_store
            .add_unique_songs_to_named_playlist(FAVORITES_PLAYLIST_NAME, &songs)
        {
            Ok(summary) => {
                self.logger
                    .info(Self::favorites_add_feedback(&summary, &songs));
                self.rebuild_left_panel();
                self.refresh_open_playlist_by_name(&summary.playlist_name);
            }
            Err(e) => self
                .logger
                .error(format!("Failed to update favorites: {}", e)),
        }
    }

    fn remove_from_favorites(&mut self) {
        let Some(songs) = self.favorite_target_songs("No songs to unfavorite") else {
            return;
        };

        match self
            .playlist_store
            .remove_matching_songs_from_named_playlist(FAVORITES_PLAYLIST_NAME, &songs)
        {
            Ok(summary) => {
                self.logger
                    .info(Self::favorites_remove_feedback(&summary, &songs));
                self.rebuild_left_panel();
                self.refresh_open_playlist_by_name(&summary.playlist_name);
            }
            Err(e) => self
                .logger
                .error(format!("Failed to update favorites: {}", e)),
        }
    }

    fn show_history(&mut self) {
        self.center_panel.set_history(self.playback_history.clone());
        self.center_panel.show_history();
        self.focused_window = FocusedWindow::Center;
        if self.playback_history.is_empty() {
            self.logger.info("No recently played tracks".to_string());
        }
    }

    fn record_playback_history(&mut self, song: &Song) {
        if Self::history_song_is_blank(song) {
            return;
        }

        if self
            .playback_history
            .first()
            .is_some_and(|existing| Self::same_history_song(existing, song))
        {
            return;
        }

        self.playback_history
            .retain(|existing| !Self::same_history_song(existing, song));
        self.playback_history.insert(0, song.clone());
        self.playback_history.truncate(PLAYBACK_HISTORY_LIMIT);
        self.save_playback_history();

        if self.center_panel.is_showing_history() {
            self.center_panel.set_history(self.playback_history.clone());
        }
    }

    fn remove_history_item(&mut self, index: usize) {
        if index >= self.playback_history.len() {
            self.logger
                .info("History item no longer exists".to_string());
            self.center_panel.set_history(self.playback_history.clone());
            return;
        }

        let removed = self.playback_history.remove(index);
        self.save_playback_history();
        self.center_panel.set_history(self.playback_history.clone());
        self.logger.info(format!(
            "Removed {} from history",
            Self::song_feedback_title(&removed)
        ));
    }

    fn clear_history(&mut self) {
        if self.playback_history.is_empty() {
            self.logger.info("No history to clear".to_string());
            self.center_panel.set_history(Vec::new());
            return;
        }

        let removed = self.playback_history.len();
        self.playback_history.clear();
        self.save_playback_history();
        self.center_panel.set_history(Vec::new());
        let noun = if removed == 1 {
            "history track"
        } else {
            "history tracks"
        };
        self.logger.info(format!("Cleared {} {}", removed, noun));
    }

    fn save_playback_history(&mut self) {
        if let Err(e) = self.history_store.save(&self.playback_history) {
            self.logger
                .error(format!("Failed to save playback history: {}", e));
        }
    }

    fn history_song_is_blank(song: &Song) -> bool {
        song.title.trim().is_empty()
            && song.artist.trim().is_empty()
            && song.album_name.trim().is_empty()
            && song.path.as_os_str().is_empty()
            && song.url.as_deref().is_none_or(|url| url.trim().is_empty())
            && song
                .stream_track_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
    }

    fn same_history_song(left: &Song, right: &Song) -> bool {
        match (
            left.stream_service.as_deref(),
            left.stream_track_id.as_deref(),
            right.stream_service.as_deref(),
            right.stream_track_id.as_deref(),
        ) {
            (Some(left_service), Some(left_id), Some(right_service), Some(right_id))
                if !left_service.trim().is_empty()
                    && !left_id.trim().is_empty()
                    && left_service
                        .trim()
                        .eq_ignore_ascii_case(right_service.trim())
                    && left_id.trim() == right_id.trim() =>
            {
                return true;
            }
            _ => {}
        }

        if !left.path.as_os_str().is_empty()
            && !right.path.as_os_str().is_empty()
            && left.path == right.path
        {
            return true;
        }

        match (left.url.as_deref(), right.url.as_deref()) {
            (Some(left_url), Some(right_url))
                if !left_url.trim().is_empty() && left_url.trim() == right_url.trim() =>
            {
                return true;
            }
            _ => {}
        }

        !left.title.trim().is_empty()
            && left.title.trim().eq_ignore_ascii_case(right.title.trim())
            && left.artist.trim().eq_ignore_ascii_case(right.artist.trim())
            && left
                .album_name
                .trim()
                .eq_ignore_ascii_case(right.album_name.trim())
    }

    fn save_current_volume_as_startup(&mut self) {
        let volume = self.player.get_playback_info().volume.min(100) as u16;
        self.config.audio.default_volume = volume;
        match self.config.save() {
            Ok(()) => {
                self.settings_panel.update_config(&self.config);
                self.logger
                    .info(format!("Startup volume saved as {}%", volume));
            }
            Err(e) => self
                .logger
                .error(format!("Failed to save startup volume: {}", e)),
        }
    }

    fn clear_queued_tracks(&mut self) {
        let queue_len = self.player.get_queue().len();
        let current_position = self.player.get_queue_position();
        let mut removed = 0;

        for index in (0..queue_len).rev() {
            if index == current_position {
                continue;
            }

            match self.player.remove_from_queue(index) {
                Ok(()) => removed += 1,
                Err(e) => {
                    self.logger.error(format!("Cannot clear queue: {}", e));
                    break;
                }
            }
        }

        if removed == 0 {
            self.logger.info("No queued tracks to clear".to_string());
        } else {
            let noun = if removed == 1 {
                "queued track"
            } else {
                "queued tracks"
            };
            self.logger.info(format!("Cleared {} {}", removed, noun));
        }

        if removed > 0 {
            self.save_queue_state();
        }
        let queue = self.player.get_queue().to_vec();
        let pos = self.player.get_queue_position();
        self.center_panel.set_queue(queue, pos);
    }

    fn save_queue_to_playlist(&mut self) {
        let songs = self.player.get_queue().to_vec();
        if songs.is_empty() {
            self.logger.info("No queued tracks to save".to_string());
            return;
        }

        self.open_playlist_picker(songs);
    }

    fn move_queue_item(&mut self, from: usize, to: usize) {
        let queue = self.player.get_queue();
        let current_position = self.player.get_queue_position();

        if from == current_position || to == current_position {
            if let Some(title) = queue.get(current_position).map(Self::song_feedback_title) {
                self.logger.info(format!("Cannot move: {}", title));
            } else {
                self.logger
                    .info("Cannot move currently playing track".to_string());
            }
            return;
        }

        if from >= queue.len() || to >= queue.len() || from == to {
            self.logger
                .info("Cannot move queue item further".to_string());
            return;
        }

        let moved_title = queue.get(from).map(Self::song_feedback_title);
        let direction = if to < from { "up" } else { "down" };

        match self.player.move_in_queue(from, to) {
            Ok(()) => {
                if let Some(title) = moved_title {
                    self.logger.info(format!("Moved {} {}", title, direction));
                } else {
                    self.logger.info("Moved queue item".to_string());
                }
                self.save_queue_state();
                let queue = self.player.get_queue().to_vec();
                let pos = self.player.get_queue_position();
                self.center_panel.set_queue(queue, pos);
                self.center_panel.select_queue_index(to);
            }
            Err(e) => self.logger.error(format!("Cannot move: {}", e)),
        }
    }

    fn move_playlist_track(&mut self, path: std::path::PathBuf, from: usize, to: usize) {
        if !PlaylistStore::is_playlist_path(&path) {
            self.logger
                .info("Open a playlist to reorder tracks".to_string());
            return;
        }

        let songs = self.center_panel.get_songs();
        if from >= songs.len() || to >= songs.len() || from == to {
            self.logger
                .info("Cannot move playlist track further".to_string());
            return;
        }

        let direction = if to < from { "up" } else { "down" };

        match self
            .playlist_store
            .move_song_in_path(path.clone(), from, to)
        {
            Ok(Some((playlist_name, moved_song, songs))) => {
                self.logger.info(format!(
                    "Moved {} {} in '{}'",
                    Self::song_feedback_title(&moved_song),
                    direction,
                    playlist_name
                ));
                let title = format!("{} ({})", playlist_name, track_count_label(songs.len()));
                self.center_panel.set_album_with_title(path, title, songs);
                self.center_panel.select_song_index(to);
                self.rebuild_left_panel();
            }
            Ok(None) => {
                self.logger.info("Playlist no longer exists".to_string());
                self.center_panel.clear_album_if_path(&path);
                self.rebuild_left_panel();
            }
            Err(e) => self
                .logger
                .error(format!("Failed to update playlist: {}", e)),
        }
    }

    fn jump_to_queue_index(&mut self, index: usize) {
        let queue = self.player.get_queue().to_vec();
        if index >= queue.len() {
            self.logger.info("No queue track selected".to_string());
            return;
        }

        match self.player.play_album(queue, index) {
            Ok(()) => {
                self.save_queue_state();
                self.log_current_track("Playing");
            }
            Err(e) => self.logger.error(format!("Playback error: {}", e)),
        }
    }

    pub fn execute(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit(),
            Action::SwitchPanel => self.focused_window = self.focused_window.next(),
            Action::ToggleSettings => self.toggle_settings(),
            Action::OpenKeybinds => self.open_keybinds(),
            Action::SelectAlbum => {
                if let Some((path, songs)) = self.left_panel.get_selected_album() {
                    if let Some(title) = self.left_panel.selected_item_label() {
                        self.center_panel.set_album_with_title(path, title, songs);
                    } else {
                        self.center_panel.set_album(path, songs);
                    }
                } else {
                    self.log_empty_left_selection();
                }
            }
            Action::PlaySelected => {
                if self.center_panel.is_showing_queue() {
                    if let Some(index) = self.center_panel.selected_queue_index() {
                        self.jump_to_queue_index(index);
                    } else {
                        self.logger.info("No queue track selected".to_string());
                    }
                } else if self.center_panel.is_showing_history() {
                    if let Some(index) = self.center_panel.selected_history_index() {
                        let songs = self.center_panel.get_history_songs();
                        self.play_album_from(songs, index);
                    } else {
                        self.logger.info("No history track selected".to_string());
                    }
                } else if self.center_panel.is_showing_album_tracks()
                    && self.search_source.is_some()
                {
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
                } else {
                    self.logger.info("Select a song first".to_string());
                }
            }
            Action::TogglePause => {
                let playback_state = self.player.get_playback_info().state.clone();
                if playback_state == PlaybackState::Stopped {
                    self.logger.info("Nothing playing".to_string());
                    return;
                }

                let next_state = match playback_state {
                    PlaybackState::Playing => "Paused",
                    PlaybackState::Paused => "Playing",
                    PlaybackState::Stopped => "Nothing playing",
                };
                match self.player.toggle_pause() {
                    Ok(()) => self.logger.info(next_state.to_string()),
                    Err(e) => self.logger.error(format!("Could not toggle pause: {}", e)),
                }
            }
            Action::NextTrack => {
                if self.player.get_playback_info().state == PlaybackState::Stopped {
                    self.logger.info("Nothing playing".to_string());
                    return;
                }

                match self.player.next() {
                    Ok(()) => {
                        self.save_queue_state();
                        self.sync_queue_view();
                        self.log_current_track("Playing");
                    }
                    Err(e) => self.logger.error(format!("Could not next track: {}", e)),
                }
            }
            Action::PreviousTrack => {
                if self.player.get_playback_info().state == PlaybackState::Stopped {
                    self.logger.info("Nothing playing".to_string());
                    return;
                }

                match self.player.previous() {
                    Ok(()) => {
                        self.save_queue_state();
                        self.sync_queue_view();
                        self.log_current_track("Playing");
                    }
                    Err(e) => self
                        .logger
                        .error(format!("Could not previous track: {}", e)),
                }
            }
            Action::StopPlayback => {
                if self.player.get_playback_info().state == PlaybackState::Stopped {
                    self.logger.info("Nothing playing".to_string());
                    return;
                }

                match self.player.stop() {
                    Ok(()) => self.logger.info("Stopped playback".to_string()),
                    Err(e) => self.logger.error(format!("Could not stop playback: {}", e)),
                }
            }
            Action::SeekForward(secs) => {
                let info = self.player.get_playback_info();
                if info.state == PlaybackState::Stopped {
                    self.logger.info("Nothing playing".to_string());
                    return;
                }
                let position = Self::bounded_seek_position(info.position + secs, info.duration);
                match self.player.seek(position) {
                    Ok(()) => self.logger.info(format!(
                        "Seeked to {}",
                        Self::format_playback_position(position)
                    )),
                    Err(e) => self.logger.error(format!("Could not seek forward: {}", e)),
                }
            }
            Action::SeekBackward(secs) => {
                let info = self.player.get_playback_info();
                if info.state == PlaybackState::Stopped {
                    self.logger.info("Nothing playing".to_string());
                    return;
                }
                let position = Self::bounded_seek_position(info.position - secs, info.duration);
                match self.player.seek(position) {
                    Ok(()) => self.logger.info(format!(
                        "Seeked to {}",
                        Self::format_playback_position(position)
                    )),
                    Err(e) => self.logger.error(format!("Could not seek backward: {}", e)),
                }
            }
            Action::VolumeUp(amount) => {
                let info = self.player.get_playback_info();
                let volume = info.volume.saturating_add(amount).min(100);
                match self.player.set_volume(volume) {
                    Ok(()) => {
                        if volume > 0 {
                            self.muted_volume_before_zero = None;
                        }
                        self.logger.info(format!("Volume {}%", volume));
                    }
                    Err(e) => self.logger.error(format!("Could not volume up: {}", e)),
                }
            }
            Action::VolumeDown(amount) => {
                let info = self.player.get_playback_info();
                let volume = info.volume.saturating_sub(amount);
                match self.player.set_volume(volume) {
                    Ok(()) => self.logger.info(format!("Volume {}%", volume)),
                    Err(e) => self.logger.error(format!("Could not volume down: {}", e)),
                }
            }
            Action::ToggleMute => {
                let current = self.player.get_playback_info().volume;
                let target = if current > 0 {
                    self.muted_volume_before_zero = Some(current);
                    0
                } else {
                    self.muted_volume_before_zero
                        .take()
                        .unwrap_or_else(|| (self.config.audio.default_volume.min(100) as u8).max(1))
                };

                match self.player.set_volume(target) {
                    Ok(()) if target == 0 => self.logger.info("Muted".to_string()),
                    Ok(()) => self.logger.info(format!("Volume {}%", target)),
                    Err(e) => self.logger.error(format!("Could not toggle mute: {}", e)),
                }
            }
            Action::SaveCurrentVolumeAsStartup => self.save_current_volume_as_startup(),
            Action::ToggleShuffle => {
                let shuffle = self.player.get_playback_info().shuffle;
                let next_shuffle = match shuffle {
                    crate::players::ShuffleMode::Off => crate::players::ShuffleMode::On,
                    crate::players::ShuffleMode::On => crate::players::ShuffleMode::Off,
                };
                let result = self.player.toggle_shuffle();
                match result {
                    Ok(()) => self.logger.info(format!("Shuffle {:?}", next_shuffle)),
                    Err(e) => self
                        .logger
                        .error(format!("Could not toggle shuffle: {}", e)),
                }
            }
            Action::CycleRepeat => {
                let repeat = self.player.get_playback_info().repeat.cycle();
                let result = self.player.cycle_repeat();
                match result {
                    Ok(()) => self.logger.info(format!("Repeat {}", repeat.label())),
                    Err(e) => self.logger.error(format!("Could not cycle repeat: {}", e)),
                }
            }
            Action::OpenLeftFilter => {
                if self.focused_window == FocusedWindow::Left
                    && self.left_panel.can_filter_active_tab()
                {
                    self.left_panel.open_filter();
                } else {
                    self.logger
                        .info("Select Local or Playlists first".to_string());
                }
            }
            Action::PlaySelectedCollection => {
                self.play_selected_collection();
            }
            Action::EnqueueSelected => {
                if self.search_source.is_some()
                    && (self.center_panel.is_showing_album_tracks()
                        || self.center_panel.is_showing_search_results())
                {
                    self.enqueue_search_result();
                } else {
                    let songs = self.center_panel.selected_songs_for_playlist();
                    if let Some(song) = songs.first() {
                        if song.has_stream_reference() {
                            let _ = self.resolve_stream_song(song, true);
                            return;
                        }
                    }

                    if !songs.is_empty() {
                        let feedback = Self::queue_feedback(&songs);
                        if let Err(e) = self.player.enqueue(songs) {
                            self.logger.error(format!("Enqueue error: {}", e));
                        } else {
                            self.save_queue_state();
                            self.logger.info(feedback);
                        }
                    } else {
                        self.logger.info("Select a song first".to_string());
                    }
                }
            }
            Action::EnqueueOpenCollection => {
                self.enqueue_open_collection();
            }
            Action::EnqueueSelectedCollection => {
                self.enqueue_selected_collection();
            }
            Action::ShowQueue => {
                let queue = self.player.get_queue().to_vec();
                let pos = self.player.get_queue_position();
                self.center_panel.set_queue(queue, pos);
                self.center_panel.show_queue();
                self.focused_window = FocusedWindow::Center;
            }
            Action::ShowHistory => {
                self.show_history();
            }
            Action::RefreshLibrary => {
                self.rebuild_left_panel_with_local_discovery(true);
                let refreshed_local_album = self.refresh_open_local_album();
                let refreshed_local_library = self.refresh_open_local_library();
                if !refreshed_local_album && !refreshed_local_library {
                    self.clear_stale_open_local_album();
                }
                self.logger.info("Library refreshed".to_string());
            }
            Action::CreatePlaylist => {
                if self.left_panel.active_tab_name() == "Playlists" {
                    self.pending_playlist_add_songs.clear();
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_create_playlist();
                } else {
                    self.logger
                        .info("Switch to the Playlists tab first".to_string());
                }
            }
            Action::RenamePlaylist => {
                if self.left_panel.active_tab_name() == "Playlists" {
                    self.open_rename_selected_playlist();
                } else {
                    self.logger
                        .info("Switch to the Playlists tab first".to_string());
                }
            }
            Action::DuplicatePlaylist => {
                if self.left_panel.active_tab_name() == "Playlists" {
                    self.open_duplicate_selected_playlist();
                } else {
                    self.logger
                        .info("Switch to the Playlists tab first".to_string());
                }
            }
            Action::DeletePlaylist => {
                if self.left_panel.active_tab_name() == "Playlists" {
                    self.delete_selected_playlist();
                } else {
                    self.logger
                        .info("Switch to the Playlists tab first".to_string());
                }
            }
            Action::AddToPlaylist => {
                let songs = self.center_panel.selected_songs_for_playlist();
                self.open_playlist_picker(songs);
            }
            Action::AddOpenCollectionToPlaylist => {
                self.add_open_collection_to_playlist();
            }
            Action::AddCurrentTrackToPlaylist => {
                self.add_current_track_to_playlist();
            }
            Action::AddToFavorites => {
                self.add_to_favorites();
            }
            Action::RemoveFromFavorites => {
                self.remove_from_favorites();
            }
            Action::OpenSearch => {
                let tab = self.left_panel.active_tab_name();
                if tab == "Qobuz" || tab == "Tidal" {
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_search();
                } else if self.center_panel.can_open_local_filter() {
                    // Local tab with songs loaded — open local filter search
                    self.focused_window = FocusedWindow::Center;
                    self.center_panel.open_search_local();
                } else if tab == "Local" && self.config.local.sources.is_empty() {
                    self.logger.info("No local sources: Settings".to_string());
                } else if tab == "Local" && !self.config.local.sources.is_empty() {
                    let songs = self.local_library_songs();
                    if songs.is_empty() {
                        self.logger
                            .info("No local songs found. Check Settings sources.".to_string());
                    } else {
                        self.focused_window = FocusedWindow::Center;
                        self.center_panel.open_search_local_library(songs);
                    }
                } else {
                    self.logger
                        .info("No songs loaded. Select an album first.".to_string());
                }
            }
        }
        self.sync_queue_view();
    }

    pub fn delegate_key_to_panel(&mut self, key: KeyEvent) {
        if self.settings_panel.opened {
            self.settings_panel.handle_events(key);
            if !self.settings_panel.opened {
                self.restore_focus_after_settings();
            }
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

    fn sync_config_from_settings(&mut self) {
        if let Some(mut new_config) = self.settings_panel.take_config_update() {
            // The settings UI does not own Tidal auth state. Preserve the live
            // credentials even if a stale settings copy emits a config update.
            let clear_tidal = self.settings_panel.take_tidal_clear_requested();
            if !clear_tidal {
                new_config.tidal = self.config.tidal.clone();
            }
            if let (Some(new_qobuz), Some(current_qobuz)) =
                (new_config.qobuz.as_mut(), self.config.qobuz.as_ref())
            {
                new_qobuz.app_id = current_qobuz.app_id.clone();
                new_qobuz.app_secret = current_qobuz.app_secret.clone();
            }

            let local_changed = self.config.local != new_config.local;
            let stream_quality_changed =
                self.config.audio.max_stream_quality != new_config.audio.max_stream_quality;
            let removed_local_source_paths: Vec<_> = if local_changed {
                self.config
                    .local
                    .sources
                    .iter()
                    .filter(|source| {
                        !new_config.local.sources.iter().any(|new_source| {
                            Self::paths_equivalent(&new_source.path, &source.path)
                        })
                    })
                    .map(|source| source.path.clone())
                    .collect()
            } else {
                Vec::new()
            };
            // Recreate Qobuz if credentials changed
            let qobuz_changed = self.config.qobuz != new_config.qobuz;
            let tidal_changed = self.config.tidal != new_config.tidal;
            if clear_tidal {
                self.pending_auth_service = None;
                self.deferred_search = None;
                self.search_source = None;
                self.center_panel.set_status(None);

                self.streaming
                    .reset_service(StreamingServiceId::Qobuz, None);
                let tidal = Some(Box::new(TidalSource::new(
                    TidalConfig::default(),
                    new_config.audio.max_stream_quality,
                )) as Box<dyn StreamingService>);
                self.streaming
                    .reset_service(StreamingServiceId::Tidal, tidal);
            } else if qobuz_changed || stream_quality_changed {
                let qobuz = new_config
                    .qobuz
                    .as_ref()
                    .filter(|q| q.has_credentials())
                    .map(|q| {
                        Box::new(QobuzSource::with_credentials(
                            q.app_id.clone(),
                            q.app_secret.clone(),
                            q.email.clone(),
                            q.password.clone(),
                            new_config.audio.max_stream_quality,
                        )) as Box<dyn StreamingService>
                    });
                self.streaming.replace_qobuz(qobuz);
            }
            if !clear_tidal && (tidal_changed || stream_quality_changed) {
                let tidal_cfg = new_config.tidal.clone().unwrap_or_default();
                let tidal = Some(Box::new(TidalSource::new(
                    tidal_cfg,
                    new_config.audio.max_stream_quality,
                )) as Box<dyn StreamingService>);
                self.streaming.replace_tidal(tidal);
            }
            self.config = new_config;
            if local_changed {
                self.rebuild_left_panel();
                self.left_panel.select_tab_by_name("Local");
                self.previous_focus_before_settings = Some(FocusedWindow::Left);
                let refreshed_local_library = self.refresh_open_local_library();
                let refreshed_local_album =
                    !refreshed_local_library && self.refresh_open_local_album();
                if !refreshed_local_library && !refreshed_local_album {
                    self.center_panel
                        .clear_album_if_under_any_path(&removed_local_source_paths);
                }
            }
            self.settings_panel.update_config(&self.config);
        }
    }

    fn sync_qobuz_auth_request_from_settings(&mut self) {
        if !self.settings_panel.take_qobuz_auth_requested() {
            return;
        }

        if !self
            .config
            .qobuz
            .as_ref()
            .is_some_and(crate::config::QobuzConfig::has_credentials)
        {
            self.settings_panel.set_qobuz_status_message(
                Some("Enter Qobuz email and password first".to_string()),
                true,
            );
            return;
        }

        self.deferred_search = None;
        self.submit_streaming_request(StreamingServiceId::Qobuz, StreamingRequest::Authenticate);
    }

    fn sync_tidal_auth_request_from_settings(&mut self) {
        if !self.settings_panel.take_tidal_auth_requested() {
            return;
        }

        self.deferred_search = None;
        self.submit_streaming_request(StreamingServiceId::Tidal, StreamingRequest::Authenticate);
    }

    fn rebuild_left_panel(&mut self) {
        self.rebuild_left_panel_with_local_discovery(false);
    }

    fn rebuild_left_panel_with_local_discovery(&mut self, force_local_discovery: bool) {
        let active_tab = self.left_panel.active_tab_name();
        let sources = Self::sources_for_config(
            &self.config,
            self.playlist_store.clone(),
            force_local_discovery,
        );
        let mut left_panel = LeftPanel::new(sources, self.logger.clone());
        left_panel.select_tab_by_name(&active_tab);
        self.left_panel = left_panel;
    }

    fn refresh_open_playlist_by_name(&mut self, playlist_name: &str) {
        let path = PlaylistStore::path_for_name(playlist_name);
        if self.center_panel.selected_album_path() != Some(&path) {
            return;
        }

        let songs = self.playlist_store.songs_for_name(playlist_name);
        let title = format!("{} ({})", playlist_name, track_count_label(songs.len()));
        self.center_panel.set_album_with_title(path, title, songs);
    }

    fn refresh_open_local_album(&mut self) -> bool {
        let Some(open_path) = self.center_panel.selected_album_path().cloned() else {
            return false;
        };

        let Some((refreshed_path, title, songs)) =
            LocalFiles::album_for_path(&self.config.local.sources, &open_path)
        else {
            return false;
        };

        self.center_panel
            .refresh_album_if_path_with_title(&open_path, refreshed_path, title, songs)
    }

    fn refresh_open_local_library(&mut self) -> bool {
        if !self.center_panel.is_showing_local_library() {
            return false;
        }

        let songs = self.local_library_songs();
        self.center_panel.refresh_local_library_if_open(songs)
    }

    fn clear_stale_open_local_album(&mut self) {
        let Some(open_path) = self.center_panel.selected_album_path().cloned() else {
            return;
        };
        if PlaylistStore::is_playlist_path(&open_path) {
            return;
        }
        if !self.open_path_belongs_to_local_source(&open_path) {
            return;
        }
        if LocalFiles::album_for_path(&self.config.local.sources, &open_path).is_some() {
            return;
        }

        self.center_panel.clear_album_if_path(&open_path);
    }

    fn open_path_belongs_to_local_source(&self, open_path: &Path) -> bool {
        self.config
            .local
            .sources
            .iter()
            .any(|source| Self::path_is_under(&source.path, open_path))
    }

    fn path_is_under(parent: &Path, child: &Path) -> bool {
        if child.starts_with(parent) {
            return true;
        }

        matches!(
            (parent.canonicalize(), child.canonicalize()),
            (Ok(parent), Ok(child)) if child.starts_with(&parent)
        )
    }

    fn toggle_settings(&mut self) {
        if self.settings_panel.opened {
            self.settings_panel.close();
            self.restore_focus_after_settings();
        } else {
            self.previous_focus_before_settings = Some(match self.focused_window {
                FocusedWindow::Settings => FocusedWindow::Left,
                other => other,
            });
            self.settings_panel.toggle_open();
            self.focused_window = FocusedWindow::Settings;
        }
    }

    fn open_keybinds(&mut self) {
        if !self.settings_panel.opened {
            self.previous_focus_before_settings = Some(match self.focused_window {
                FocusedWindow::Settings => FocusedWindow::Left,
                other => other,
            });
            self.settings_panel.toggle_open();
        }
        self.settings_panel
            .select_tab(crate::ui::settings::SettingsTab::Keybinds);
        self.focused_window = FocusedWindow::Settings;
    }

    fn restore_focus_after_settings(&mut self) {
        self.focused_window = self
            .previous_focus_before_settings
            .take()
            .filter(|focus| *focus != FocusedWindow::Settings)
            .unwrap_or(FocusedWindow::Left);
    }

    fn sync_queue_view(&mut self) {
        if self.center_panel.is_showing_queue() {
            let queue = self.player.get_queue().to_vec();
            let pos = self.player.get_queue_position();
            self.center_panel.set_queue(queue, pos);
        }
    }

    fn local_library_songs(&self) -> Vec<Song> {
        self.config
            .get_local_sources()
            .into_iter()
            .flat_map(|source| LocalFiles::songs_from_path(source.path))
            .collect()
    }

    fn paths_equivalent(left: &Path, right: &Path) -> bool {
        if left == right {
            return true;
        }

        matches!(
            (left.canonicalize(), right.canonicalize()),
            (Ok(left), Ok(right)) if left == right
        )
    }

    fn poll_pending_auth(&mut self) {
        let service_id = match self.pending_auth_service {
            Some(id) => id,
            None => return,
        };

        if self.streaming.is_busy(service_id) {
            return;
        }

        self.submit_streaming_request(service_id, StreamingRequest::PollAuth);
    }

    /// Check if a service's credentials have changed and persist them if so.
    /// Called after every background task completes to catch token refreshes.
    fn sync_service_credentials(&mut self, service_id: StreamingServiceId) {
        match service_id {
            StreamingServiceId::Tidal => {
                if let Some(data) = self.streaming.persist_data(service_id) {
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
            StreamingServiceId::Qobuz => {
                if let Some((app_id, app_secret)) = self.streaming.app_credentials(service_id) {
                    if let Some(ref qobuz_config) = self.config.qobuz {
                        if qobuz_config.app_id != app_id || qobuz_config.app_secret != app_secret {
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

    fn persist_streaming_credentials(&mut self, service_id: StreamingServiceId) {
        match service_id {
            StreamingServiceId::Qobuz => {
                if let Some((app_id, app_secret)) = self.streaming.app_credentials(service_id) {
                    if let Some(ref mut qobuz_config) = self.config.qobuz {
                        qobuz_config.app_id = app_id;
                        qobuz_config.app_secret = app_secret;
                    }
                }
            }
            StreamingServiceId::Tidal => {
                if let Some(data) = self.streaming.persist_data(service_id) {
                    if let Ok(tidal_cfg) = serde_json::from_str::<TidalConfig>(&data) {
                        self.config.tidal = Some(tidal_cfg);
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

    fn submit_streaming_request(
        &mut self,
        service_id: StreamingServiceId,
        request: StreamingRequest,
    ) {
        match self.streaming.submit(service_id, request, &self.config) {
            StreamingSubmitResult::Started { status } => {
                self.center_panel.set_status(Some(status));
            }
            StreamingSubmitResult::ReplacedSearch { status } => {
                self.center_panel.set_status(Some(status));
                self.logger
                    .info("Replacing in-flight search...".to_string());
            }
            StreamingSubmitResult::Queued { status } => {
                self.center_panel.set_status(Some(status));
            }
            StreamingSubmitResult::Unavailable { status } => {
                self.center_panel.set_status(Some(status.clone()));
                self.logger.info(status);
            }
            StreamingSubmitResult::Busy => {
                self.logger
                    .info("Streaming request already in progress...".to_string());
            }
        }
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

        let search_mode = self.center_panel.search_mode();
        self.logger.info(format!(
            "Searching {} {} for '{}'...",
            service_id.as_str(),
            search_mode.label(),
            query
        ));

        let request = match search_mode {
            SearchMode::Albums => StreamingRequest::SearchAlbums {
                query: query.to_string(),
                limit: 20,
            },
            SearchMode::Artists => StreamingRequest::SearchArtists {
                query: query.to_string(),
                limit: 20,
            },
            SearchMode::Tracks => StreamingRequest::SearchTracks {
                query: query.to_string(),
                limit: 20,
            },
        };
        self.submit_streaming_request(service_id, request);
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

        self.logger
            .info(format!("Loading tracks for '{}'...", album.title));

        self.submit_streaming_request(
            service_id,
            StreamingRequest::GetAlbumTracks {
                album_id: album.id,
                album_title: format!("{} - {}", album.artist, album.title),
            },
        );
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

        self.logger
            .info(format!("Loading albums for '{}'...", artist.name));

        self.submit_streaming_request(
            service_id,
            StreamingRequest::GetArtistAlbums {
                artist_id: artist.id,
                artist_name: artist.name,
            },
        );
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

        // If we're in album tracks mode and have multiple tracks, play the whole album
        if self.center_panel.is_showing_album_tracks() && self.search_results.len() > 1 {
            let tracks = self.search_results.clone();
            self.logger
                .info(format!("Resolving {} album tracks...", tracks.len()));
            self.submit_streaming_request(
                service_id,
                StreamingRequest::PlayAlbumStream {
                    tracks,
                    start_index: index,
                },
            );
            return;
        }

        let track = match self.search_results.get(index) {
            Some(t) => t.clone(),
            None => return,
        };

        self.logger
            .info(format!("Getting stream for {}...", track.display_title()));

        let source_song = Self::song_from_stream_track(&track, service_id);
        self.submit_streaming_request(
            service_id,
            StreamingRequest::GetStreamUrl {
                track_id: track.id,
                title: source_song.title.clone(),
                enqueue: false,
                source_song: Some(Box::new(source_song)),
            },
        );
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

        let source_song = Self::song_from_stream_track(&track, service_id);
        self.submit_streaming_request(
            service_id,
            StreamingRequest::GetStreamUrl {
                title: source_song.title.clone(),
                track_id: track.id,
                enqueue: true,
                source_song: Some(Box::new(source_song)),
            },
        );
    }

    fn play_album_from(&mut self, songs: Vec<Song>, index: usize) {
        let Some(song) = songs.get(index).cloned() else {
            return;
        };

        if songs.iter().any(Song::has_stream_reference) {
            if !song.has_stream_reference() {
                self.start_sequential_mixed_playlist_playback(songs, index);
                return;
            }

            if let Some(service_id) = Self::stream_service_for_playlist(&songs) {
                self.logger
                    .info(format!("Resolving {} playlist tracks...", songs.len()));
                self.submit_streaming_request(
                    service_id,
                    StreamingRequest::PlayMixedPlaylist {
                        songs,
                        start_index: index,
                    },
                );
                return;
            }

            self.start_sequential_mixed_playlist_playback(songs, index);
            return;
        }

        match self.player.play_album(songs, index) {
            Ok(()) => {
                self.save_queue_state();
                self.log_current_track("Playing");
            }
            Err(e) => self.logger.error(format!("Playback error: {}", e)),
        }
    }

    fn start_sequential_mixed_playlist_playback(&mut self, songs: Vec<Song>, start_index: usize) {
        let mut ordered_songs = Self::playlist_play_order(&songs, start_index);
        let Some(first_song) = ordered_songs.pop_front() else {
            return;
        };

        self.pending_playlist_queue = ordered_songs;
        self.pending_playlist_failed_count = 0;
        self.pending_playlist_active = true;

        if first_song.has_stream_reference() {
            self.logger.info(format!(
                "Resolving {} playlist tracks...",
                self.pending_playlist_queue.len() + 1
            ));
            if !self.resolve_stream_song(&first_song, false) {
                self.cancel_pending_playlist_playback();
            }
            return;
        }

        if let Err(e) = self.player.play(&first_song) {
            self.logger.error(format!("Playback error: {}", e));
            self.cancel_pending_playlist_playback();
        } else {
            self.save_queue_state();
            self.log_current_track("Playing");
            self.enqueue_next_pending_playlist_track();
        }
    }

    fn playlist_play_order(songs: &[Song], start_index: usize) -> VecDeque<Song> {
        let mut ordered_songs = VecDeque::with_capacity(songs.len());
        if start_index >= songs.len() {
            return ordered_songs;
        }

        ordered_songs.extend(songs[start_index..].iter().cloned());
        ordered_songs.extend(songs[..start_index].iter().cloned());
        ordered_songs
    }

    fn enqueue_next_pending_playlist_track(&mut self) {
        while let Some(song) = self.pending_playlist_queue.pop_front() {
            if song.has_stream_reference() {
                if self.resolve_stream_song(&song, true) {
                    return;
                }
                self.pending_playlist_failed_count += 1;
                continue;
            }

            if let Err(e) = self.player.enqueue(vec![song]) {
                self.logger.error(format!("Enqueue error: {}", e));
            } else {
                self.save_queue_state();
            }
        }

        self.finish_pending_playlist_playback();
    }

    fn finish_pending_playlist_playback(&mut self) {
        self.pending_playlist_active = false;
        if self.pending_playlist_failed_count > 0 {
            self.logger.info(format!(
                "{} could not be resolved",
                track_count_label(self.pending_playlist_failed_count)
            ));
            self.pending_playlist_failed_count = 0;
        }
    }

    fn cancel_pending_playlist_playback(&mut self) {
        self.pending_playlist_queue.clear();
        self.pending_playlist_failed_count = 0;
        self.pending_playlist_active = false;
    }

    fn stream_service_for_playlist(songs: &[Song]) -> Option<StreamingServiceId> {
        let mut service_id = None;

        for song in songs {
            if !song.has_stream_reference() {
                continue;
            }

            let service = StreamingServiceId::from_tab_name(song.stream_service.as_deref()?)?;
            if let Some(existing) = service_id {
                if existing != service {
                    return None;
                }
            } else {
                service_id = Some(service);
            }
        }

        service_id
    }

    fn resolve_stream_song(&mut self, song: &Song, enqueue: bool) -> bool {
        let (Some(service_name), Some(track_id)) = (
            song.stream_service.as_deref(),
            song.stream_track_id.as_deref(),
        ) else {
            self.logger
                .error("Could not resolve saved stream metadata".to_string());
            return false;
        };

        let Some(service_id) = StreamingServiceId::from_tab_name(service_name) else {
            self.logger
                .error(format!("Unknown streaming service '{}'", service_name));
            return false;
        };

        self.logger
            .info(format!("Getting stream for {}...", song.title));
        self.submit_streaming_request(
            service_id,
            StreamingRequest::GetStreamUrl {
                track_id: track_id.to_string(),
                title: song.title.clone(),
                enqueue,
                source_song: Some(Box::new(song.clone())),
            },
        );
        true
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
                if info.state != PlaybackState::Stopped {
                    if let Some(song) = info.current_song.as_ref() {
                        self.record_playback_history(song);
                    }
                }
                self.right_panel.update_playback_info(info);
                self.save_queue_state_if_marker_changed();
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

    fn log_current_track(&mut self, prefix: &str) {
        if let Some(song) = self.player.get_playback_info().current_song.clone() {
            self.record_playback_history(&song);
            self.logger
                .info(format!("{} {}", prefix, Self::song_feedback_title(&song)));
        } else {
            self.logger.info("No current track".to_string());
        }
    }

    fn queue_feedback(songs: &[Song]) -> String {
        match songs {
            [] => "No queued tracks".to_string(),
            [song] => format!("Queued {}", Self::song_feedback_title(song)),
            songs => format!("Queued {}", track_count_label(songs.len())),
        }
    }

    fn playlist_add_feedback(playlist_name: &str, songs: &[Song], song_count: usize) -> String {
        match songs {
            [song] => format!(
                "Added {} to '{}'",
                Self::song_feedback_title(song),
                playlist_name
            ),
            _ => format!(
                "Added {} to '{}'",
                track_count_label(song_count),
                playlist_name
            ),
        }
    }

    fn favorites_add_feedback(summary: &PlaylistAddSummary, songs: &[Song]) -> String {
        if summary.added_count == 0 {
            return match songs {
                [song] => format!(
                    "{} is already in {}",
                    Self::song_feedback_title(song),
                    summary.playlist_name
                ),
                _ => format!("Selected tracks are already in {}", summary.playlist_name),
            };
        }

        if summary.skipped_count > 0 {
            return format!(
                "Added {} to {} ({} already there)",
                track_count_label(summary.added_count),
                summary.playlist_name,
                summary.skipped_count
            );
        }

        match songs {
            [song] => format!(
                "Added {} to {}",
                Self::song_feedback_title(song),
                summary.playlist_name
            ),
            _ => format!(
                "Added {} to {}",
                track_count_label(summary.added_count),
                summary.playlist_name
            ),
        }
    }

    fn favorites_remove_feedback(summary: &PlaylistRemoveSummary, songs: &[Song]) -> String {
        if !summary.existed || summary.removed_count == 0 {
            return match songs {
                [song] => format!(
                    "{} is not in {}",
                    Self::song_feedback_title(song),
                    summary.playlist_name
                ),
                _ => format!("Selected tracks are not in {}", summary.playlist_name),
            };
        }

        if summary.missed_count > 0 {
            return format!(
                "Removed {} from {} ({} not found)",
                track_count_label(summary.removed_count),
                summary.playlist_name,
                summary.missed_count
            );
        }

        match songs {
            [song] => format!(
                "Removed {} from {}",
                Self::song_feedback_title(song),
                summary.playlist_name
            ),
            _ => format!(
                "Removed {} from {}",
                track_count_label(summary.removed_count),
                summary.playlist_name
            ),
        }
    }

    fn song_feedback_title(song: &Song) -> String {
        match (song.artist.trim(), song.title.trim()) {
            ("", "") => song.path.to_string_lossy().into_owned(),
            ("", title) => title.to_string(),
            (artist, "") => artist.to_string(),
            (artist, title) => format!("{} - {}", artist, title),
        }
    }

    fn format_playback_position(position: f64) -> String {
        let total_seconds = position.max(0.0).floor() as u64;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;

        if hours > 0 {
            format!("{}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{}:{:02}", minutes, seconds)
        }
    }

    fn bounded_seek_position(position: f64, duration: f64) -> f64 {
        let position = position.max(0.0);
        if duration.is_finite() && duration > 0.0 {
            position.min(duration)
        } else {
            position
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

    fn poll_streaming_task_results(&mut self) {
        loop {
            let events = self.streaming.poll_events();
            if events.is_empty() {
                break;
            }
            for event in events {
                self.handle_streaming_event(event);
            }
        }
    }

    fn handle_streaming_event(&mut self, event: StreamingCoordinatorEvent) {
        match event {
            StreamingCoordinatorEvent::Status(status) => self.center_panel.set_status(status),
            StreamingCoordinatorEvent::ServiceReturned(service_id) => {
                self.sync_service_credentials(service_id);
            }
            StreamingCoordinatorEvent::Output { service_id, output } => {
                self.handle_streaming_task_output(service_id, *output);
            }
            StreamingCoordinatorEvent::TimedOut {
                service_id,
                timeout,
            } => {
                self.logger.error(format!(
                    "{} request timed out after {}s",
                    service_id.as_str(),
                    timeout.as_secs()
                ));
            }
        }
    }

    fn handle_streaming_task_output(
        &mut self,
        service_id: StreamingServiceId,
        output: StreamingTaskOutput,
    ) {
        match output {
            StreamingTaskOutput::AlbumSearchResults(albums) => {
                self.logger.info(format!("Found {} albums", albums.len()));
                let display_titles: Vec<String> =
                    albums.iter().map(|a| a.display_title()).collect();
                self.album_results = albums;
                self.search_source = Some(service_id);
                self.center_panel.set_album_results(display_titles);
            }
            StreamingTaskOutput::AlbumTracks {
                album_title,
                tracks,
            } => {
                self.logger
                    .info(format!("Loaded {}", track_count_label(tracks.len())));
                let songs: Vec<Song> = tracks
                    .iter()
                    .map(|t| Song {
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        album_name: t.album.clone(),
                        stream_service: Some(service_id.as_str().to_string()),
                        stream_track_id: Some(t.id.clone()),
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
                self.pending_auth_service = Some(service_id);
                self.deferred_search = deferred_query;
                self.center_panel.set_status(Some(message.clone()));
                match service_id {
                    StreamingServiceId::Qobuz => self
                        .settings_panel
                        .set_qobuz_status_message(Some(message.clone()), false),
                    StreamingServiceId::Tidal => self
                        .settings_panel
                        .set_tidal_status_message(Some(message.clone()), false),
                }
                self.logger.info(message);
            }
            StreamingTaskOutput::AuthCompleted => {
                self.logger
                    .info(format!("Authenticated with {}", service_id.as_str()));
                self.pending_auth_service = None;
                self.persist_streaming_credentials(service_id);
                match service_id {
                    StreamingServiceId::Qobuz => self.settings_panel.set_qobuz_status_message(
                        Some("Qobuz account verified".to_string()),
                        false,
                    ),
                    StreamingServiceId::Tidal => self
                        .settings_panel
                        .set_tidal_status_message(Some("Tidal account saved".to_string()), false),
                }
                if let Some(query) = self.deferred_search.take() {
                    self.perform_search(&query);
                }
            }
            StreamingTaskOutput::PollPending => {}
            StreamingTaskOutput::StreamUrlResult {
                title,
                stream,
                enqueue,
                source_song,
            } => match stream {
                Some(stream) => {
                    let song = Self::song_from_stream_result(title, stream, source_song);
                    if enqueue {
                        let feedback = Self::queue_feedback(std::slice::from_ref(&song));
                        if let Err(e) = self.player.enqueue(vec![song]) {
                            self.logger.error(format!("Enqueue error: {}", e));
                        } else {
                            self.save_queue_state();
                            self.logger.info(feedback);
                        }
                        if self.pending_playlist_active {
                            self.enqueue_next_pending_playlist_track();
                        }
                    } else if let Err(e) = self.player.play(&song) {
                        self.logger.error(format!("Playback error: {}", e));
                        if self.pending_playlist_active {
                            self.cancel_pending_playlist_playback();
                        }
                    } else {
                        self.save_queue_state();
                        self.log_current_track("Playing");
                        if self.pending_playlist_active {
                            self.enqueue_next_pending_playlist_track();
                        }
                    }
                }
                None => {
                    self.logger
                        .error("Could not get stream URL for this track".to_string());
                    if self.pending_playlist_active {
                        if enqueue {
                            self.pending_playlist_failed_count += 1;
                            self.enqueue_next_pending_playlist_track();
                        } else {
                            self.cancel_pending_playlist_playback();
                        }
                    }
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
                    } else {
                        self.save_queue_state();
                        self.log_current_track("Playing");
                        if !remaining_songs.is_empty() {
                            if let Err(e) = self.player.enqueue(remaining_songs) {
                                self.logger.error(format!("Enqueue error: {}", e));
                            } else {
                                self.save_queue_state();
                            }
                        }
                    }
                } else {
                    self.logger
                        .error("Could not resolve the selected track".to_string());
                }
                if failed_count > 0 {
                    self.logger.info(format!(
                        "{} could not be resolved",
                        track_count_label(failed_count)
                    ));
                }
            }
            StreamingTaskOutput::ArtistSearchResults(artists) => {
                self.logger.info(format!("Found {} artists", artists.len()));
                let display_titles: Vec<String> =
                    artists.iter().map(|a| a.display_title()).collect();
                self.artist_results = artists;
                self.search_source = Some(service_id);
                self.center_panel.set_artist_results(display_titles);
            }
            StreamingTaskOutput::TrackSearchResults(tracks) => {
                self.logger
                    .info(format!("Found {}", track_count_label(tracks.len())));
                let songs: Vec<Song> = tracks
                    .iter()
                    .map(|t| Self::song_from_stream_track(t, service_id))
                    .collect();
                self.search_results = tracks;
                self.search_source = Some(service_id);
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
                match service_id {
                    StreamingServiceId::Qobuz => self
                        .settings_panel
                        .set_qobuz_status_message(Some(msg.clone()), true),
                    StreamingServiceId::Tidal => self
                        .settings_panel
                        .set_tidal_status_message(Some(msg.clone()), true),
                }
                self.logger.error(msg);
            }
        }
    }

    fn song_from_stream_result(
        title: String,
        stream: ResolvedStream,
        source_song: Option<Box<Song>>,
    ) -> Song {
        let ResolvedStream {
            source,
            quality_label,
        } = stream;
        let mut song = match source {
            ResolvedStreamSource::Url(url) => Song::from_url(title, url, quality_label),
            ResolvedStreamSource::Manifest {
                contents,
                file_extension,
            } => Song::from_manifest(title, contents, file_extension, quality_label),
        };

        if let Some(source_song) = source_song {
            let source_song = *source_song;
            song.artist = source_song.artist;
            song.album_name = source_song.album_name;
            song.disc_number = source_song.disc_number;
            song.track_number = source_song.track_number;
            song.duration_secs = source_song.duration_secs;
            song.stream_service = source_song.stream_service;
            song.stream_track_id = source_song.stream_track_id;
        }

        song
    }

    fn song_from_stream_track(track: &StreamTrack, service_id: StreamingServiceId) -> Song {
        Song {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album_name: track.album.clone(),
            stream_service: Some(service_id.as_str().to_string()),
            stream_track_id: Some(track.id.clone()),
            ..Default::default()
        }
    }

    fn check_active_streaming_task_timeout(&mut self) {
        for event in self.streaming.check_timeouts(&self.config) {
            self.handle_streaming_event(event);
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

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        config::{AudioConfig, Config, LocalConfig, MaxStreamQuality, QobuzConfig, TidalConfig},
        players::{
            MusicPlayer, PlaybackInfo, PlaybackState, PlayerResult, RepeatMode, ShuffleMode,
        },
        playlist::PlaylistStore,
        sources::{
            local::{reset_song_scan_count, song_scan_count},
            song::Song,
        },
    };

    use super::{App, FocusedWindow};
    use crate::action::Action;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rmus-app-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
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
                default_shuffle: ShuffleMode::Off,
                default_repeat: RepeatMode::Off,
            },
        }
    }

    #[derive(Debug)]
    struct VolumeTestPlayer {
        info: PlaybackInfo,
        queue: Vec<Song>,
    }

    impl VolumeTestPlayer {
        fn new(volume: u8) -> Self {
            Self {
                info: PlaybackInfo {
                    volume,
                    ..Default::default()
                },
                queue: Vec::new(),
            }
        }
    }

    impl MusicPlayer for VolumeTestPlayer {
        fn play(&mut self, song: &Song) -> PlayerResult<()> {
            self.info.current_song = Some(song.clone());
            self.queue = vec![song.clone()];
            Ok(())
        }

        fn play_album(&mut self, songs: Vec<Song>, start_index: usize) -> PlayerResult<()> {
            self.queue = songs;
            self.info.current_song = self.queue.get(start_index).cloned();
            Ok(())
        }

        fn toggle_pause(&mut self) -> PlayerResult<()> {
            Ok(())
        }

        fn stop(&mut self) -> PlayerResult<()> {
            self.info.current_song = None;
            Ok(())
        }

        fn next(&mut self) -> PlayerResult<()> {
            Ok(())
        }

        fn previous(&mut self) -> PlayerResult<()> {
            Ok(())
        }

        fn seek(&mut self, position: f64) -> PlayerResult<()> {
            self.info.position = position;
            Ok(())
        }

        fn set_volume(&mut self, volume: u8) -> PlayerResult<()> {
            self.info.volume = volume;
            Ok(())
        }

        fn poll(&mut self) -> PlayerResult<PlaybackInfo> {
            Ok(self.info.clone())
        }

        fn get_playback_info(&self) -> &PlaybackInfo {
            &self.info
        }

        fn is_alive(&self) -> bool {
            true
        }

        fn shutdown(&mut self) -> PlayerResult<()> {
            Ok(())
        }

        fn toggle_shuffle(&mut self) -> PlayerResult<()> {
            Ok(())
        }

        fn cycle_repeat(&mut self) -> PlayerResult<()> {
            Ok(())
        }

        fn enqueue(&mut self, songs: Vec<Song>) -> PlayerResult<()> {
            self.queue.extend(songs);
            Ok(())
        }

        fn restore_queue(&mut self, songs: Vec<Song>, position: usize) -> PlayerResult<()> {
            self.queue = songs;
            self.info.current_song = None;
            self.info.state = PlaybackState::Stopped;
            self.info.position = 0.0;
            let _ = position;
            Ok(())
        }

        fn get_queue(&self) -> &[Song] {
            &self.queue
        }

        fn get_queue_position(&self) -> usize {
            0
        }

        fn remove_from_queue(&mut self, index: usize) -> PlayerResult<()> {
            if index < self.queue.len() {
                self.queue.remove(index);
            }
            Ok(())
        }

        fn move_in_queue(&mut self, _from: usize, _to: usize) -> PlayerResult<()> {
            Ok(())
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

    #[test]
    fn account_save_after_source_update_preserves_local_sources() {
        let dir = temp_dir("account-after-source");
        let mut app = App::new_for_test(default_config(), None, None);

        app.settings_panel.toggle_open();
        app.settings_panel.handle_events(key(KeyCode::Char('a')));
        for c in "Library".chars() {
            app.settings_panel.handle_events(key(KeyCode::Char(c)));
        }
        app.settings_panel.handle_events(key(KeyCode::Tab));
        for c in dir.to_string_lossy().chars() {
            app.settings_panel.handle_events(key(KeyCode::Char(c)));
        }
        app.settings_panel.handle_events(key(KeyCode::Enter));
        app.sync_config_from_settings();

        assert_eq!(app.config.local.sources.len(), 1);

        app.settings_panel.handle_events(key(KeyCode::Tab));
        app.settings_panel.handle_events(key(KeyCode::Char('e')));
        for c in "user@example.com".chars() {
            app.settings_panel.handle_events(key(KeyCode::Char(c)));
        }
        app.settings_panel.handle_events(key(KeyCode::Tab));
        for c in "secret".chars() {
            app.settings_panel.handle_events(key(KeyCode::Char(c)));
        }
        app.settings_panel.handle_events(key(KeyCode::Enter));
        app.sync_config_from_settings();

        assert_eq!(app.config.local.sources.len(), 1);
        assert_eq!(app.config.local.sources[0].name, "Library");
        assert_eq!(
            app.config.local.sources[0].path,
            dir.canonicalize().unwrap()
        );
        let qobuz = app.config.qobuz.expect("qobuz account should be saved");
        assert_eq!(qobuz.email, "user@example.com");
        assert_eq!(qobuz.password, "secret");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn source_update_does_not_scan_all_local_songs_when_library_view_is_closed() {
        let dir = temp_dir("lazy-source-update");
        for index in 0..20 {
            fs::write(dir.join(format!("{index:02} - Track.flac")), "").unwrap();
        }
        let mut app = App::new_for_test(default_config(), None, None);

        app.settings_panel.toggle_open();
        app.settings_panel.handle_events(key(KeyCode::Char('a')));
        for c in "Large Library".chars() {
            app.settings_panel.handle_events(key(KeyCode::Char(c)));
        }
        app.settings_panel.handle_events(key(KeyCode::Tab));
        for c in dir.to_string_lossy().chars() {
            app.settings_panel.handle_events(key(KeyCode::Char(c)));
        }
        app.settings_panel.handle_events(key(KeyCode::Enter));

        reset_song_scan_count();
        app.sync_config_from_settings();

        assert_eq!(
            song_scan_count(),
            0,
            "saving a local source should rebuild the list without eagerly parsing every track"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn configured_default_volume_is_applied_to_player() {
        let mut config = default_config();
        config.audio.default_volume = 74;

        let app = App::new_for_test(config, None, None);

        assert_eq!(app.player.get_playback_info().volume, 74);
    }

    #[test]
    fn configured_playback_mode_defaults_are_applied_to_player() {
        let mut config = default_config();
        config.audio.default_shuffle = ShuffleMode::On;
        config.audio.default_repeat = RepeatMode::All;

        let app = App::new_for_test(config, None, None);

        let info = app.player.get_playback_info();
        assert_eq!(info.shuffle, ShuffleMode::On);
        assert_eq!(info.repeat, RepeatMode::All);
    }

    #[test]
    fn saving_current_volume_updates_startup_volume() {
        let mut app = App::new_for_test_with_playlist_store_and_player(
            default_config(),
            None,
            None,
            PlaylistStore::default(),
            Box::new(VolumeTestPlayer::new(65)),
        );

        app.execute(Action::SaveCurrentVolumeAsStartup);

        assert_eq!(app.config.audio.default_volume, 65);
    }

    #[test]
    fn playlist_copy_name_skips_existing_copy_names() {
        let names = vec![
            "Road".to_string(),
            "Road Copy".to_string(),
            "road copy 2".to_string(),
        ];

        assert_eq!(App::playlist_copy_name("Road", &names), "Road Copy 3");
    }

    #[test]
    fn account_clear_removes_streaming_credentials() {
        let mut config = default_config();
        config.qobuz = Some(QobuzConfig {
            email: "user@example.com".to_string(),
            password: "secret".to_string(),
            app_id: "app".to_string(),
            app_secret: "secret".to_string(),
        });
        config.tidal = Some(TidalConfig {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            country_code: "US".to_string(),
            token_expiry: 1_900_000_000,
        });
        let mut app = App::new_for_test(config, None, None);

        app.settings_panel.toggle_open();
        app.settings_panel.handle_events(key(KeyCode::Tab));
        app.settings_panel.handle_events(key(KeyCode::Char('c')));
        app.sync_config_from_settings();

        assert!(app.config.qobuz.is_none());
        assert!(app.config.tidal.is_none());
    }

    #[test]
    fn blank_qobuz_account_save_keeps_qobuz_unconfigured() {
        let mut app = App::new_for_test(default_config(), None, None);

        app.settings_panel.toggle_open();
        app.settings_panel.handle_events(key(KeyCode::Tab));
        app.settings_panel.handle_events(key(KeyCode::Char('e')));
        app.settings_panel.handle_events(key(KeyCode::Enter));
        app.sync_config_from_settings();

        assert!(app.config.qobuz.is_none());
    }

    #[test]
    fn toggling_settings_focuses_overlay_and_restores_previous_focus() {
        let mut app = App::new_for_test(default_config(), None, None);
        app.focused_window = FocusedWindow::Center;

        app.execute(Action::ToggleSettings);

        assert!(app.settings_panel.opened);
        assert_eq!(app.focused_window, FocusedWindow::Settings);

        app.execute(Action::ToggleSettings);

        assert!(!app.settings_panel.opened);
        assert_eq!(app.focused_window, FocusedWindow::Center);
    }

    #[test]
    fn closing_settings_with_escape_restores_previous_focus() {
        let mut app = App::new_for_test(default_config(), None, None);
        app.focused_window = FocusedWindow::Right;

        app.execute(Action::ToggleSettings);
        app.delegate_key_to_panel(key(KeyCode::Esc));

        assert!(!app.settings_panel.opened);
        assert_eq!(app.focused_window, FocusedWindow::Right);
    }
}
