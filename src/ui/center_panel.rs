use std::{collections::VecDeque, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{
    playlist::PlaylistStore,
    sources::song::Song,
    ui::{input_line::InputLine, theme, widget::selected_row_style},
    utils::track_count_label,
};

const PAGE_STEP: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Albums,
    Artists,
    Tracks,
}

impl SearchMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Albums => Self::Artists,
            Self::Artists => Self::Tracks,
            Self::Tracks => Self::Albums,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Tracks => "Tracks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CenterPanelMode {
    Album,
    SearchInput,
    SearchResults,
    AlbumResults,
    AlbumTracks,
    Queue,
    History,
    ArtistResults,
    CreatePlaylist,
    RenamePlaylist,
    DuplicatePlaylist,
    PlaylistPicker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CenterPanelEvent {
    QuerySubmitted(String),
    SongSelected,
    AlbumSelected(usize),
    ArtistSelected(usize),
    QueueItemRemoved(usize),
    QueueItemJumped(usize),
    QueueItemMoved {
        from: usize,
        to: usize,
    },
    QueueCurrentItemRemovalBlocked,
    QueueClearRequested,
    QueueSaveRequested,
    HistoryItemRemoved(usize),
    HistoryClearRequested,
    PlaylistCreated(String),
    PlaylistRenamed(String),
    PlaylistRenameCancelled,
    PlaylistDuplicated(String),
    PlaylistDuplicateCancelled,
    PlaylistSelectedForAdd(usize),
    PlaylistAddCancelled,
    PlaylistTrackRemoved {
        path: PathBuf,
        track_index: usize,
    },
    PlaylistTrackMoved {
        path: PathBuf,
        from: usize,
        to: usize,
    },
}

const LOCAL_LIBRARY_TITLE: &str = "Local Library";

#[derive(Debug)]
pub struct CenterPanel {
    selected_album: Option<PathBuf>,
    selected_album_title: Option<String>,
    songs: Vec<Song>,
    /// Unfiltered album songs, used to restore after local search filtering.
    album_songs: Vec<Song>,
    list_state: ListState,
    mode: CenterPanelMode,
    search_input: InputLine,
    events: VecDeque<CenterPanelEvent>,
    status_message: Option<String>,
    /// Album titles for display in AlbumResults mode.
    album_display_titles: Vec<String>,
    album_list_state: ListState,
    /// The album title we're viewing tracks for (shown in AlbumTracks title bar).
    viewing_album_title: Option<String>,
    /// Queue songs (populated from player state).
    queue_songs: Vec<Song>,
    /// Currently playing position in the queue.
    queue_position: usize,
    queue_list_state: ListState,
    /// Visible queue rows after filtering, mapped back to original queue indices.
    queue_visible_indices: Vec<usize>,
    queue_filter_input: InputLine,
    /// The mode to return to when closing the queue view.
    pre_queue_mode: Option<CenterPanelMode>,
    /// Recently played songs, most recent first.
    history_songs: Vec<Song>,
    history_list_state: ListState,
    /// Visible history rows after filtering, mapped back to original history indices.
    history_visible_indices: Vec<usize>,
    history_filter_input: InputLine,
    /// The mode to return to when closing the history view.
    pre_history_mode: Option<CenterPanelMode>,
    /// Current search mode (Albums/Artists/Tracks).
    search_mode: SearchMode,
    /// Whether search input is filtering the currently loaded local album.
    local_filter_mode: bool,
    /// Artist display titles for ArtistResults mode.
    artist_display_titles: Vec<String>,
    artist_list_state: ListState,
    /// Input for creating a new playlist name.
    playlist_name_input: InputLine,
    /// Inline feedback for playlist create/rename failures.
    playlist_status_message: Option<String>,
    /// Playlist names for the picker overlay.
    playlist_picker_names: Vec<String>,
    playlist_picker_state: ListState,
    /// The mode to return to when closing playlist create/picker.
    pre_playlist_mode: Option<CenterPanelMode>,
    /// Currently playing song, used to mark matching rows while browsing.
    current_song: Option<Song>,
    /// Songs in the Favorites playlist, used to mark matching rows while browsing.
    favorite_songs: Vec<Song>,
}

impl CenterPanel {
    pub fn new() -> Self {
        Self {
            selected_album: None,
            selected_album_title: None,
            songs: Vec::new(),
            album_songs: Vec::new(),
            list_state: ListState::default(),
            mode: CenterPanelMode::Album,
            search_input: InputLine::new(),
            events: VecDeque::new(),
            status_message: None,
            album_display_titles: Vec::new(),
            album_list_state: ListState::default(),
            viewing_album_title: None,
            queue_songs: Vec::new(),
            queue_position: 0,
            queue_list_state: ListState::default(),
            queue_visible_indices: Vec::new(),
            queue_filter_input: InputLine::new(),
            pre_queue_mode: None,
            history_songs: Vec::new(),
            history_list_state: ListState::default(),
            history_visible_indices: Vec::new(),
            history_filter_input: InputLine::new(),
            pre_history_mode: None,
            search_mode: SearchMode::default(),
            local_filter_mode: false,
            artist_display_titles: Vec::new(),
            artist_list_state: ListState::default(),
            playlist_name_input: InputLine::new(),
            playlist_status_message: None,
            playlist_picker_names: Vec::new(),
            playlist_picker_state: ListState::default(),
            pre_playlist_mode: None,
            current_song: None,
            favorite_songs: Vec::new(),
        }
    }

    pub fn set_status(&mut self, message: Option<String>) {
        self.status_message = message;
    }

    pub fn set_current_song(&mut self, song: Option<Song>) {
        self.current_song = song;
    }

    pub fn set_favorite_songs(&mut self, songs: Vec<Song>) {
        self.favorite_songs = songs;
    }

    pub fn set_album(&mut self, path: PathBuf, songs: Vec<Song>) {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Songs")
            .to_string();
        self.set_album_with_title(path, title, songs);
    }

    pub fn set_album_with_title(&mut self, path: PathBuf, title: String, songs: Vec<Song>) {
        self.selected_album = Some(path);
        self.selected_album_title = Some(title);
        self.local_filter_mode = false;
        self.album_songs = songs.clone();
        self.songs = songs;
        self.mode = CenterPanelMode::Album;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn selected_album_path(&self) -> Option<&PathBuf> {
        self.selected_album.as_ref()
    }

    pub fn is_showing_local_library(&self) -> bool {
        self.selected_album.is_none()
            && self.selected_album_title.as_deref() == Some(LOCAL_LIBRARY_TITLE)
    }

    pub fn refresh_local_library_if_open(&mut self, songs: Vec<Song>) -> bool {
        if !self.is_showing_local_library() {
            return false;
        }

        self.album_songs = songs.clone();
        if self.local_filter_mode
            && matches!(
                self.mode,
                CenterPanelMode::SearchInput | CenterPanelMode::SearchResults
            )
        {
            let query = self.search_input.value.clone();
            self.apply_local_filter(&query);
        } else {
            self.songs = songs;
            if !self.songs.is_empty() {
                let index = self
                    .list_state
                    .selected()
                    .unwrap_or(0)
                    .min(self.songs.len() - 1);
                self.list_state.select(Some(index));
            } else {
                self.list_state.select(None);
            }
        }

        true
    }

    pub fn refresh_album_if_path(&mut self, path: &PathBuf, songs: Vec<Song>) -> bool {
        if self.selected_album.as_ref() != Some(path) {
            return false;
        }

        self.refresh_album_songs(songs);
        true
    }

    pub fn refresh_album_if_path_with_title(
        &mut self,
        current_path: &PathBuf,
        refreshed_path: PathBuf,
        title: String,
        songs: Vec<Song>,
    ) -> bool {
        if self.selected_album.as_ref() != Some(current_path) {
            return false;
        }

        self.selected_album = Some(refreshed_path);
        self.selected_album_title = Some(title);
        self.refresh_album_songs(songs);
        true
    }

    fn refresh_album_songs(&mut self, songs: Vec<Song>) {
        self.album_songs = songs.clone();
        if self.local_filter_mode
            && matches!(
                self.mode,
                CenterPanelMode::SearchInput | CenterPanelMode::SearchResults
            )
        {
            let query = self.search_input.value.clone();
            self.apply_local_filter(&query);
        } else if self.mode == CenterPanelMode::Album
            || (self.local_filter_mode && self.mode == CenterPanelMode::SearchInput)
        {
            self.songs = songs;
            if !self.songs.is_empty() {
                let index = self
                    .list_state
                    .selected()
                    .unwrap_or(0)
                    .min(self.songs.len() - 1);
                self.list_state.select(Some(index));
            } else {
                self.list_state.select(None);
            }
        }
    }

    pub fn clear_album_if_path(&mut self, path: &PathBuf) {
        if self.selected_album.as_ref() != Some(path) {
            return;
        }

        self.clear_album();
    }

    pub fn clear_album_if_under_any_path(&mut self, paths: &[PathBuf]) {
        let Some(selected_album) = self.selected_album.as_ref() else {
            return;
        };

        if paths.iter().any(|path| selected_album.starts_with(path)) {
            self.clear_album();
        }
    }

    fn clear_album(&mut self) {
        self.selected_album = None;
        self.selected_album_title = None;
        self.songs.clear();
        self.album_songs.clear();
        self.list_state.select(None);
        self.local_filter_mode = false;
        if matches!(
            self.mode,
            CenterPanelMode::Album | CenterPanelMode::SearchInput | CenterPanelMode::SearchResults
        ) {
            self.mode = CenterPanelMode::Album;
        }
    }

    pub fn set_search_results(&mut self, songs: Vec<Song>) {
        self.local_filter_mode = false;
        self.songs = songs;
        self.mode = CenterPanelMode::SearchResults;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn set_album_results(&mut self, display_titles: Vec<String>) {
        self.local_filter_mode = false;
        self.album_display_titles = display_titles;
        self.mode = CenterPanelMode::AlbumResults;
        if !self.album_display_titles.is_empty() {
            self.album_list_state.select(Some(0));
        } else {
            self.album_list_state.select(None);
        }
    }

    pub fn set_album_tracks(&mut self, album_title: String, songs: Vec<Song>) {
        self.local_filter_mode = false;
        self.viewing_album_title = Some(album_title);
        self.songs = songs;
        self.mode = CenterPanelMode::AlbumTracks;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn set_queue(&mut self, songs: Vec<Song>, position: usize) {
        let selected_original_index = self.selected_queue_index();
        self.queue_songs = songs;
        self.queue_position = position;
        self.refresh_queue_visible_indices(selected_original_index);
    }

    pub fn set_history(&mut self, songs: Vec<Song>) {
        let selected_original_index = self.selected_history_index();
        self.history_songs = songs;
        self.refresh_history_visible_indices(selected_original_index);
    }

    fn refresh_queue_visible_indices(&mut self, preferred_original_index: Option<usize>) {
        let query = self.queue_filter_input.value.trim().to_lowercase();
        self.queue_visible_indices = self
            .queue_songs
            .iter()
            .enumerate()
            .filter_map(|(index, song)| {
                if query.is_empty() || Self::song_matches_text_filter(song, &query) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        self.select_preferred_queue_index(preferred_original_index);
    }

    fn select_preferred_queue_index(&mut self, preferred_original_index: Option<usize>) {
        if self.queue_visible_indices.is_empty() {
            self.queue_list_state.select(None);
            return;
        }

        let visible_index = preferred_original_index
            .filter(|index| *index < self.queue_songs.len())
            .and_then(|index| self.visible_queue_index_for_original(index))
            .or_else(|| self.visible_queue_index_for_original(self.queue_position))
            .unwrap_or(0);
        self.queue_list_state.select(Some(visible_index));
    }

    fn visible_queue_index_for_original(&self, original_index: usize) -> Option<usize> {
        self.queue_visible_indices
            .iter()
            .position(|index| *index == original_index)
    }

    fn refresh_history_visible_indices(&mut self, preferred_original_index: Option<usize>) {
        let query = self.history_filter_input.value.trim().to_lowercase();
        self.history_visible_indices = self
            .history_songs
            .iter()
            .enumerate()
            .filter_map(|(index, song)| {
                if query.is_empty() || Self::song_matches_text_filter(song, &query) {
                    Some(index)
                } else {
                    None
                }
            })
            .collect();

        self.select_preferred_history_index(preferred_original_index);
    }

    fn select_preferred_history_index(&mut self, preferred_original_index: Option<usize>) {
        if self.history_visible_indices.is_empty() {
            self.history_list_state.select(None);
            return;
        }

        let visible_index = preferred_original_index
            .filter(|index| *index < self.history_songs.len())
            .and_then(|index| self.visible_history_index_for_original(index))
            .unwrap_or(0);
        self.history_list_state.select(Some(visible_index));
    }

    fn visible_history_index_for_original(&self, original_index: usize) -> Option<usize> {
        self.history_visible_indices
            .iter()
            .position(|index| *index == original_index)
    }

    pub fn reset_queue_selection_to_current(&mut self) {
        self.select_preferred_queue_index(Some(self.queue_position));
    }

    pub fn show_queue(&mut self) {
        if self.mode != CenterPanelMode::Queue {
            self.pre_queue_mode = Some(self.mode);
        }
        self.mode = CenterPanelMode::Queue;
        self.reset_queue_selection_to_current();
    }

    pub fn is_showing_queue(&self) -> bool {
        self.mode == CenterPanelMode::Queue
    }

    pub fn show_history(&mut self) {
        if self.mode != CenterPanelMode::History {
            self.pre_history_mode = Some(self.mode);
        }
        self.mode = CenterPanelMode::History;
        self.select_preferred_history_index(self.selected_history_index());
    }

    pub fn is_showing_history(&self) -> bool {
        self.mode == CenterPanelMode::History
    }

    pub fn selected_history_index(&self) -> Option<usize> {
        self.history_list_state
            .selected()
            .and_then(|index| self.history_visible_indices.get(index))
            .copied()
    }

    pub fn get_history_songs(&self) -> Vec<Song> {
        self.history_songs.clone()
    }

    pub fn selected_queue_index(&self) -> Option<usize> {
        self.queue_list_state
            .selected()
            .and_then(|index| self.queue_visible_indices.get(index))
            .copied()
    }

    pub fn select_queue_index(&mut self, index: usize) {
        if let Some(visible_index) = self.visible_queue_index_for_original(index) {
            self.queue_list_state.select(Some(visible_index));
        }
    }

    fn open_queue_filter(&mut self) {
        let selected_original_index = self.selected_queue_index();
        self.queue_filter_input.enter_input_mode();
        self.refresh_queue_visible_indices(selected_original_index);
    }

    fn clear_queue_filter(&mut self) {
        let selected_original_index = self.selected_queue_index();
        self.queue_filter_input.exit_input_mode();
        self.refresh_queue_visible_indices(selected_original_index);
    }

    fn has_queue_filter_query(&self) -> bool {
        !self.queue_filter_input.value.trim().is_empty()
    }

    fn open_history_filter(&mut self) {
        let selected_original_index = self.selected_history_index();
        self.history_filter_input.enter_input_mode();
        self.refresh_history_visible_indices(selected_original_index);
    }

    fn clear_history_filter(&mut self) {
        let selected_original_index = self.selected_history_index();
        self.history_filter_input.exit_input_mode();
        self.refresh_history_visible_indices(selected_original_index);
    }

    fn has_history_filter_query(&self) -> bool {
        !self.history_filter_input.value.trim().is_empty()
    }

    pub fn search_mode(&self) -> SearchMode {
        self.search_mode
    }

    pub fn set_artist_results(&mut self, display_titles: Vec<String>) {
        self.local_filter_mode = false;
        self.artist_display_titles = display_titles;
        self.mode = CenterPanelMode::ArtistResults;
        if !self.artist_display_titles.is_empty() {
            self.artist_list_state.select(Some(0));
        } else {
            self.artist_list_state.select(None);
        }
    }

    pub fn open_create_playlist(&mut self) {
        self.pre_playlist_mode = Some(self.mode);
        self.mode = CenterPanelMode::CreatePlaylist;
        self.playlist_status_message = None;
        self.playlist_name_input.enter_input_mode();
    }

    pub fn open_rename_playlist(&mut self, current_name: String) {
        self.pre_playlist_mode = Some(self.mode);
        self.mode = CenterPanelMode::RenamePlaylist;
        self.playlist_status_message = None;
        self.playlist_name_input.enter_input_mode();
        self.playlist_name_input.set_value(current_name);
    }

    pub fn open_duplicate_playlist(&mut self, suggested_name: String) {
        self.pre_playlist_mode = Some(self.mode);
        self.mode = CenterPanelMode::DuplicatePlaylist;
        self.playlist_status_message = None;
        self.playlist_name_input.enter_input_mode();
        self.playlist_name_input.set_value(suggested_name);
    }

    pub fn complete_playlist_creation(&mut self) {
        self.playlist_status_message = None;
        self.playlist_name_input.exit_input_mode();
        self.mode = self
            .pre_playlist_mode
            .take()
            .unwrap_or(CenterPanelMode::Album);
    }

    pub fn complete_playlist_rename(&mut self) {
        self.playlist_status_message = None;
        self.playlist_name_input.exit_input_mode();
        self.mode = self
            .pre_playlist_mode
            .take()
            .unwrap_or(CenterPanelMode::Album);
    }

    pub fn complete_playlist_duplicate(&mut self) {
        self.playlist_status_message = None;
        self.playlist_name_input.exit_input_mode();
        self.mode = self
            .pre_playlist_mode
            .take()
            .unwrap_or(CenterPanelMode::Album);
    }

    pub fn reject_playlist_creation(&mut self, message: String) {
        self.playlist_status_message = Some(message);
        self.mode = CenterPanelMode::CreatePlaylist;
    }

    pub fn reject_playlist_rename(&mut self, message: String) {
        self.playlist_status_message = Some(message);
        self.mode = CenterPanelMode::RenamePlaylist;
    }

    pub fn reject_playlist_duplicate(&mut self, message: String) {
        self.playlist_status_message = Some(message);
        self.mode = CenterPanelMode::DuplicatePlaylist;
    }

    pub fn open_playlist_picker(&mut self, names: Vec<String>) {
        self.pre_playlist_mode = Some(self.mode);
        self.playlist_picker_names = names;
        self.mode = CenterPanelMode::PlaylistPicker;
        if !self.playlist_picker_names.is_empty() {
            self.playlist_picker_state.select(Some(0));
        }
    }

    /// Open search for streaming services — clears songs to avoid stale data.
    pub fn open_search(&mut self) {
        self.songs.clear();
        self.album_songs.clear();
        self.album_display_titles.clear();
        self.artist_display_titles.clear();
        self.status_message = None;
        self.local_filter_mode = false;
        self.list_state.select(None);
        self.album_list_state.select(None);
        self.artist_list_state.select(None);
        self.mode = CenterPanelMode::SearchInput;
        self.search_input.enter_input_mode();
    }

    /// Open search for local filtering — keeps album songs visible while typing.
    pub fn open_search_local(&mut self) {
        self.restore_album_songs();
        self.status_message = None;
        self.local_filter_mode = true;
        self.mode = CenterPanelMode::SearchInput;
        self.search_input.enter_input_mode();
    }

    pub fn open_search_local_library(&mut self, songs: Vec<Song>) {
        self.selected_album = None;
        self.selected_album_title = Some(LOCAL_LIBRARY_TITLE.to_string());
        self.album_songs = songs.clone();
        self.songs = songs;
        self.status_message = None;
        self.local_filter_mode = true;
        self.mode = CenterPanelMode::SearchInput;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
        self.search_input.enter_input_mode();
    }

    /// Filter album_songs by query and display the matches.
    pub fn filter_songs(&mut self, query: &str) {
        self.apply_local_filter(query);
        self.mode = CenterPanelMode::SearchResults;
    }

    fn apply_local_filter(&mut self, query: &str) {
        self.local_filter_mode = true;
        let query_lower = query.to_lowercase();
        self.songs = self
            .album_songs
            .iter()
            .filter(|s| Self::song_matches_text_filter(s, &query_lower))
            .cloned()
            .collect();
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    fn song_matches_text_filter(song: &Song, query_lower: &str) -> bool {
        song.title.to_lowercase().contains(query_lower)
            || song.artist.to_lowercase().contains(query_lower)
            || song.album_name.to_lowercase().contains(query_lower)
            || song
                .path
                .to_string_lossy()
                .to_lowercase()
                .contains(query_lower)
    }

    pub fn close_search(&mut self) {
        self.search_input.exit_input_mode();
        self.restore_album_songs();
        self.local_filter_mode = false;
        self.mode = CenterPanelMode::Album;
    }

    /// If we have stashed album songs (from local filtering), restore them.
    fn restore_album_songs(&mut self) {
        if !self.album_songs.is_empty() {
            self.songs = self.album_songs.clone();
            if !self.songs.is_empty() {
                self.list_state.select(Some(0));
            }
        }
    }

    pub fn is_search_input_active(&self) -> bool {
        self.mode == CenterPanelMode::SearchInput
    }

    pub fn is_text_input_active(&self) -> bool {
        matches!(
            self.mode,
            CenterPanelMode::SearchInput
                | CenterPanelMode::CreatePlaylist
                | CenterPanelMode::RenamePlaylist
                | CenterPanelMode::DuplicatePlaylist
        ) || (self.mode == CenterPanelMode::Queue && self.queue_filter_input.is_input_mode())
            || (self.mode == CenterPanelMode::History && self.history_filter_input.is_input_mode())
    }

    pub fn handles_escape(&self) -> bool {
        matches!(
            self.mode,
            CenterPanelMode::SearchInput
                | CenterPanelMode::SearchResults
                | CenterPanelMode::AlbumResults
                | CenterPanelMode::AlbumTracks
                | CenterPanelMode::Queue
                | CenterPanelMode::History
                | CenterPanelMode::ArtistResults
                | CenterPanelMode::CreatePlaylist
                | CenterPanelMode::RenamePlaylist
                | CenterPanelMode::DuplicatePlaylist
                | CenterPanelMode::PlaylistPicker
        )
    }

    pub fn is_showing_search_results(&self) -> bool {
        self.mode == CenterPanelMode::SearchResults
    }

    pub fn is_showing_album_tracks(&self) -> bool {
        self.mode == CenterPanelMode::AlbumTracks
    }

    pub fn is_showing_queueable_collection(&self) -> bool {
        match self.mode {
            CenterPanelMode::Album => self.selected_album.is_some(),
            CenterPanelMode::AlbumTracks => true,
            _ => false,
        }
    }

    pub fn next_event(&mut self) -> Option<CenterPanelEvent> {
        self.events.pop_front()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        match self.mode {
            CenterPanelMode::SearchInput => self.render_search_input(frame, area, is_focused),
            CenterPanelMode::SearchResults => self.render_search_results(frame, area, is_focused),
            CenterPanelMode::Album => self.render_album(frame, area, is_focused),
            CenterPanelMode::AlbumResults => self.render_album_results(frame, area, is_focused),
            CenterPanelMode::AlbumTracks => self.render_album_tracks(frame, area, is_focused),
            CenterPanelMode::Queue => self.render_queue(frame, area, is_focused),
            CenterPanelMode::History => self.render_history(frame, area, is_focused),
            CenterPanelMode::ArtistResults => self.render_artist_results(frame, area, is_focused),
            CenterPanelMode::CreatePlaylist => self.render_create_playlist(frame, area, is_focused),
            CenterPanelMode::RenamePlaylist => self.render_rename_playlist(frame, area, is_focused),
            CenterPanelMode::DuplicatePlaylist => {
                self.render_duplicate_playlist(frame, area, is_focused)
            }
            CenterPanelMode::PlaylistPicker => self.render_playlist_picker(frame, area, is_focused),
        }
    }

    fn render_album(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let title = Self::collection_title(
            self.selected_album_title.as_deref().unwrap_or("Songs"),
            &self.songs,
        );

        let list_items: Vec<ListItem> = if self.songs.is_empty() {
            self.empty_album_items()
        } else {
            self.songs
                .iter()
                .enumerate()
                .map(|(i, s)| self.numbered_song_list_item(i, s))
                .collect()
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn empty_album_items(&self) -> Vec<ListItem<'static>> {
        let lines: &[&str] = match self.selected_album.as_ref() {
            None => &["Select an album or playlist", "Use / to search"],
            Some(path) if PlaylistStore::is_playlist_path(path) => {
                &["Playlist is empty", "Add songs with A."]
            }
            Some(_) => &["No songs found", "Check this source folder."],
        };

        lines
            .iter()
            .map(|line| ListItem::new(*line).style(theme::muted_style()))
            .collect()
    }

    fn render_search_input(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let [input_area, list_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);

        // Search input
        let search_title = if self.local_filter_mode {
            " Filter Songs ".to_string()
        } else {
            format!(" Search {} (Tab to switch) ", self.search_mode.label())
        };
        let input_block = Block::bordered()
            .title(search_title)
            .borders(Borders::ALL)
            .border_style(theme::accent_style());

        let mut input_spans = vec![Span::styled("> ", theme::accent_bold_style())];
        input_spans.extend(self.search_input.display_spans(true, theme::accent_style()));
        let input_text = Line::from(input_spans);

        let input_paragraph = Paragraph::new(input_text).block(input_block);
        frame.render_widget(input_paragraph, input_area);

        if self.local_filter_mode {
            let list_items: Vec<ListItem> =
                self.songs.iter().map(|s| self.song_list_item(s)).collect();
            let base_title =
                Self::collection_title(&format!("Songs ({})", self.songs.len()), &self.songs);
            let title = match &self.status_message {
                Some(msg) => format!("{} - {}", base_title, msg),
                None => base_title,
            };
            let list = List::new(list_items)
                .block(
                    Block::bordered()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(border_style),
                )
                .highlight_style(selected_row_style());

            frame.render_stateful_widget(list, list_area, &mut self.list_state);
        } else {
            // Results list (from previous search, if any)
            let list_items: Vec<ListItem> = self
                .album_display_titles
                .iter()
                .map(|t| ListItem::new(t.as_str()))
                .collect();

            let list = List::new(list_items)
                .block(
                    Block::bordered()
                        .title(match &self.status_message {
                            Some(msg) => format!("Results ({})", msg),
                            None => "Results".to_string(),
                        })
                        .borders(Borders::ALL)
                        .border_style(border_style),
                )
                .highlight_style(selected_row_style());

            frame.render_stateful_widget(list, list_area, &mut self.album_list_state);
        }
    }

    fn song_row_label(song: &Song) -> String {
        let mut label = if !song.artist.is_empty() {
            format!("{} - {}", song.artist, song.title)
        } else {
            song.title.clone()
        };

        if let Some(duration) = Self::duration_label(song.duration_secs) {
            label.push_str(&format!(" ({duration})"));
        }

        let album = song.album_name.trim();
        if !album.is_empty() {
            label.push_str(&format!(" [{album}]"));
        }

        label
    }

    fn song_list_item(&self, song: &Song) -> ListItem<'static> {
        let label = self.favorite_marked_song_label(Self::song_row_label(song), song);
        self.current_marked_song_item(label, song)
    }

    fn numbered_song_row_label(index: usize, song: &Song) -> String {
        let number = match (song.disc_number, song.track_number) {
            (Some(disc), Some(track)) => format!("{disc}.{track:02}"),
            (Some(disc), None) => format!("D{disc}"),
            (None, Some(track)) => format!("{track:>2}."),
            (None, None) => format!("{:>2}.", index + 1),
        };
        format!("{} {}", number, Self::song_row_label(song))
    }

    fn numbered_song_list_item(&self, index: usize, song: &Song) -> ListItem<'static> {
        let label =
            self.favorite_marked_song_label(Self::numbered_song_row_label(index, song), song);
        self.current_marked_song_item(label, song)
    }

    fn favorite_marked_song_label(&self, label: String, song: &Song) -> String {
        if self.is_favorite_song(song) {
            format!("{label} [fav]")
        } else {
            label
        }
    }

    fn current_marked_song_item(&self, label: String, song: &Song) -> ListItem<'static> {
        if self.is_current_song(song) {
            return ListItem::new(format!("> {label}")).style(theme::current_style());
        }

        ListItem::new(label)
    }

    fn is_current_song(&self, song: &Song) -> bool {
        self.current_song
            .as_ref()
            .is_some_and(|current| Self::song_identity_matches(song, current))
    }

    fn is_favorite_song(&self, song: &Song) -> bool {
        self.favorite_songs
            .iter()
            .any(|favorite| Self::song_identity_matches(song, favorite))
    }

    fn song_identity_matches(left: &Song, right: &Song) -> bool {
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

        if let (Some(left_url), Some(right_url)) = (left.url.as_deref(), right.url.as_deref()) {
            let left_url = left_url.trim();
            let right_url = right_url.trim();
            return !left_url.is_empty() && left_url == right_url;
        }

        false
    }

    fn queue_song_row_label(
        index: usize,
        queue_len: usize,
        song: &Song,
        is_current: bool,
    ) -> String {
        let marker = if is_current { ">" } else { " " };
        let width = queue_len.max(1).to_string().len();
        format!(
            "{marker} {:>width$}. {}",
            index + 1,
            Self::song_row_label(song)
        )
    }

    fn collection_title(base: &str, songs: &[Song]) -> String {
        match Self::collection_duration_label(songs) {
            Some(duration) => format!("{base} - {duration}"),
            None => base.to_string(),
        }
    }

    fn collection_duration_label(songs: &[Song]) -> Option<String> {
        let total_duration = songs
            .iter()
            .filter_map(|song| song.duration_secs)
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .sum::<f64>();

        Self::duration_label(Some(total_duration))
    }

    fn duration_label(duration_secs: Option<f64>) -> Option<String> {
        let duration_secs = duration_secs?;
        if !duration_secs.is_finite() || duration_secs <= 0.0 {
            return None;
        }

        let total_seconds = duration_secs.round() as u64;
        let seconds = total_seconds % 60;
        let total_minutes = total_seconds / 60;
        let minutes = total_minutes % 60;
        let hours = total_minutes / 60;

        if hours > 0 {
            Some(format!("{hours}:{minutes:02}:{seconds:02}"))
        } else {
            Some(format!("{minutes}:{seconds:02}"))
        }
    }

    fn render_search_results(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let list_items: Vec<ListItem> = if self.songs.is_empty() {
            vec![
                ListItem::new("No matching songs").style(theme::muted_style()),
                ListItem::new("Press / to search again.").style(theme::muted_style()),
            ]
        } else {
            self.songs.iter().map(|s| self.song_list_item(s)).collect()
        };

        let result_count = self.songs.len();
        let base_title =
            Self::collection_title(&format!("Search Results ({})", result_count), &self.songs);
        let title = match &self.status_message {
            Some(msg) => format!("{} - {}", base_title, msg),
            None => base_title,
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_album_results(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let list_items: Vec<ListItem> = if self.album_display_titles.is_empty() {
            vec![
                ListItem::new("No matching albums").style(theme::muted_style()),
                ListItem::new("Press / to search again.").style(theme::muted_style()),
            ]
        } else {
            self.album_display_titles
                .iter()
                .map(|t| ListItem::new(t.as_str()))
                .collect()
        };

        let result_count = self.album_display_titles.len();
        let title = match &self.status_message {
            Some(msg) => format!("Albums ({}) - {}", result_count, msg),
            None => format!("Albums ({})", result_count),
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, area, &mut self.album_list_state);
    }

    fn render_album_tracks(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let list_items: Vec<ListItem> = if self.songs.is_empty() {
            vec![
                ListItem::new("No tracks found").style(theme::muted_style()),
                ListItem::new("Press Esc to return to albums.").style(theme::muted_style()),
            ]
        } else {
            self.songs
                .iter()
                .enumerate()
                .map(|(i, s)| self.numbered_song_list_item(i, s))
                .collect()
        };

        let base_title = match &self.viewing_album_title {
            Some(name) => format!("{} ({})", name, track_count_label(self.songs.len())),
            None => format!("Tracks ({})", self.songs.len()),
        };
        let title = Self::collection_title(&base_title, &self.songs);

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_queue_filter(&self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let mut input_spans = vec![Span::styled("/ ", theme::accent_bold_style())];
        input_spans.extend(self.queue_filter_input.display_spans(
            self.queue_filter_input.is_input_mode(),
            theme::accent_style(),
        ));

        let paragraph = Paragraph::new(Line::from(input_spans)).block(
            Block::bordered()
                .title(" Filter Queue ")
                .borders(Borders::ALL)
                .border_style(border_style),
        );
        frame.render_widget(paragraph, area);
    }

    fn render_queue(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);
        let show_filter = self.queue_filter_input.is_input_mode() || self.has_queue_filter_query();
        let (filter_area, list_area) = if show_filter {
            let [filter_area, list_area] =
                Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
            (Some(filter_area), list_area)
        } else {
            (None, area)
        };

        if let Some(filter_area) = filter_area {
            self.render_queue_filter(frame, filter_area, is_focused);
        }

        let list_items: Vec<ListItem> = if self.queue_songs.is_empty() {
            vec![
                ListItem::new("Queue is empty").style(theme::muted_style()),
                ListItem::new("Play or enqueue a song to fill it.").style(theme::muted_style()),
            ]
        } else if self.queue_visible_indices.is_empty() {
            vec![
                ListItem::new("No queue matches").style(theme::muted_style()),
                ListItem::new("Press Esc to clear the filter.").style(theme::muted_style()),
            ]
        } else {
            let queue_len = self.queue_songs.len();
            self.queue_visible_indices
                .iter()
                .filter_map(|index| self.queue_songs.get(*index).map(|song| (*index, song)))
                .map(|(index, song)| {
                    let is_current = index == self.queue_position;
                    let display = self.favorite_marked_song_label(
                        Self::queue_song_row_label(index, queue_len, song, is_current),
                        song,
                    );
                    let style = if is_current {
                        theme::current_style()
                    } else {
                        theme::default_style()
                    };
                    ListItem::new(display).style(style)
                })
                .collect()
        };

        let title = if self.has_queue_filter_query() {
            format!(
                "Queue ({}/{} matches)",
                self.queue_visible_indices.len(),
                self.queue_songs.len()
            )
        } else {
            Self::collection_title(
                &format!("Queue ({})", track_count_label(self.queue_songs.len())),
                &self.queue_songs,
            )
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, list_area, &mut self.queue_list_state);
    }

    fn render_history_filter(&self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let mut input_spans = vec![Span::styled("/ ", theme::accent_bold_style())];
        input_spans.extend(self.history_filter_input.display_spans(
            self.history_filter_input.is_input_mode(),
            theme::accent_style(),
        ));

        let paragraph = Paragraph::new(Line::from(input_spans)).block(
            Block::bordered()
                .title(" Filter History ")
                .borders(Borders::ALL)
                .border_style(border_style),
        );
        frame.render_widget(paragraph, area);
    }

    fn render_history(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);
        let show_filter =
            self.history_filter_input.is_input_mode() || self.has_history_filter_query();
        let (filter_area, list_area) = if show_filter {
            let [filter_area, list_area] =
                Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);
            (Some(filter_area), list_area)
        } else {
            (None, area)
        };

        if let Some(filter_area) = filter_area {
            self.render_history_filter(frame, filter_area, is_focused);
        }

        let list_items: Vec<ListItem> = if self.history_songs.is_empty() {
            vec![
                ListItem::new("No recently played tracks").style(theme::muted_style()),
                ListItem::new("Play a song to fill history.").style(theme::muted_style()),
            ]
        } else if self.history_visible_indices.is_empty() {
            vec![
                ListItem::new("No history matches").style(theme::muted_style()),
                ListItem::new("Press Esc to clear the filter.").style(theme::muted_style()),
            ]
        } else {
            self.history_visible_indices
                .iter()
                .filter_map(|index| self.history_songs.get(*index).map(|song| (*index, song)))
                .map(|(index, song)| self.numbered_song_list_item(index, song))
                .collect()
        };

        let title = if self.has_history_filter_query() {
            format!(
                "Recently Played ({}/{} matches)",
                self.history_visible_indices.len(),
                self.history_songs.len()
            )
        } else {
            Self::collection_title(
                &format!("Recently Played ({})", self.history_songs.len()),
                &self.history_songs,
            )
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, list_area, &mut self.history_list_state);
    }

    fn render_artist_results(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let list_items: Vec<ListItem> = if self.artist_display_titles.is_empty() {
            vec![
                ListItem::new("No matching artists").style(theme::muted_style()),
                ListItem::new("Press / to search again.").style(theme::muted_style()),
            ]
        } else {
            self.artist_display_titles
                .iter()
                .map(|t| ListItem::new(t.as_str()))
                .collect()
        };

        let result_count = self.artist_display_titles.len();
        let title = match &self.status_message {
            Some(msg) => format!("Artists ({}) - {}", result_count, msg),
            None => format!("Artists ({})", result_count),
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, area, &mut self.artist_list_state);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match self.mode {
            CenterPanelMode::SearchInput => self.handle_search_input(key),
            CenterPanelMode::SearchResults => self.handle_search_results(key),
            CenterPanelMode::Album => self.handle_album(key),
            CenterPanelMode::AlbumResults => self.handle_album_results(key),
            CenterPanelMode::AlbumTracks => self.handle_album_tracks(key),
            CenterPanelMode::Queue => self.handle_queue(key),
            CenterPanelMode::History => self.handle_history(key),
            CenterPanelMode::ArtistResults => self.handle_artist_results(key),
            CenterPanelMode::CreatePlaylist => self.handle_create_playlist(key),
            CenterPanelMode::RenamePlaylist => self.handle_rename_playlist(key),
            CenterPanelMode::DuplicatePlaylist => self.handle_duplicate_playlist(key),
            CenterPanelMode::PlaylistPicker => self.handle_playlist_picker(key),
        }
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Tab => {
                if !self.local_filter_mode {
                    self.search_mode = self.search_mode.cycle();
                }
            }
            KeyCode::Enter => {
                let query = self.search_input.value.trim().to_string();
                if query.is_empty() && !self.local_filter_mode {
                    self.status_message = Some("Query required".to_string());
                } else {
                    self.events
                        .push_back(CenterPanelEvent::QuerySubmitted(query));
                    self.search_input.confirm_input();
                    // Set mode based on search type so the UI shows the right view.
                    self.mode = if self.local_filter_mode {
                        CenterPanelMode::SearchResults
                    } else {
                        match self.search_mode {
                            SearchMode::Albums => CenterPanelMode::AlbumResults,
                            SearchMode::Artists => CenterPanelMode::ArtistResults,
                            SearchMode::Tracks => CenterPanelMode::SearchResults,
                        }
                    };
                }
            }
            KeyCode::Char(c) => {
                self.status_message = None;
                self.search_input.append_char(c);
                if self.local_filter_mode {
                    let query = self.search_input.value.clone();
                    self.apply_local_filter(&query);
                }
            }
            KeyCode::Backspace => {
                self.status_message = None;
                self.search_input.delete_char();
                if self.local_filter_mode {
                    let query = self.search_input.value.clone();
                    self.apply_local_filter(&query);
                }
            }
            KeyCode::Delete => {
                self.status_message = None;
                self.search_input.delete_next_char();
                if self.local_filter_mode {
                    let query = self.search_input.value.clone();
                    self.apply_local_filter(&query);
                }
            }
            KeyCode::Left => self.search_input.move_cursor_left(),
            KeyCode::Right => self.search_input.move_cursor_right(),
            KeyCode::Home => self.search_input.move_cursor_to_start(),
            KeyCode::End => self.search_input.move_cursor_to_end(),
            _ => {}
        }
    }

    fn handle_search_results(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            KeyCode::Home | KeyCode::Char('g') => {
                Self::select_first(&mut self.list_state, self.songs.len())
            }
            KeyCode::End | KeyCode::Char('G') => {
                Self::select_last(&mut self.list_state, self.songs.len())
            }
            KeyCode::PageDown => Self::select_next_page(&mut self.list_state, self.songs.len()),
            KeyCode::PageUp => Self::select_previous_page(&mut self.list_state, self.songs.len()),
            KeyCode::Enter => {
                if self.list_state.selected().is_some() {
                    self.events.push_back(CenterPanelEvent::SongSelected);
                }
            }
            KeyCode::Char('/') => {
                if self.local_filter_mode {
                    self.open_search_local();
                } else {
                    self.open_search();
                }
            }
            KeyCode::Esc => {
                self.restore_album_songs();
                self.local_filter_mode = false;
                self.mode = CenterPanelMode::Album;
            }
            _ => {}
        }
    }

    fn handle_album(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            KeyCode::Home | KeyCode::Char('g') => {
                Self::select_first(&mut self.list_state, self.songs.len())
            }
            KeyCode::End | KeyCode::Char('G') => {
                Self::select_last(&mut self.list_state, self.songs.len())
            }
            KeyCode::PageDown => Self::select_next_page(&mut self.list_state, self.songs.len()),
            KeyCode::PageUp => Self::select_previous_page(&mut self.list_state, self.songs.len()),
            KeyCode::Enter => {
                if self.list_state.selected().is_some() {
                    self.events.push_back(CenterPanelEvent::SongSelected);
                }
            }
            KeyCode::Char('d') => {
                if let (Some(path), Some(track_index)) =
                    (self.selected_album.clone(), self.list_state.selected())
                {
                    self.events
                        .push_back(CenterPanelEvent::PlaylistTrackRemoved { path, track_index });
                }
            }
            KeyCode::Char('J') => {
                if let (Some(path), Some(index)) =
                    (self.selected_album.clone(), self.list_state.selected())
                {
                    self.events.push_back(CenterPanelEvent::PlaylistTrackMoved {
                        path,
                        from: index,
                        to: index.saturating_add(1),
                    });
                }
            }
            KeyCode::Char('K') => {
                if let (Some(path), Some(index)) =
                    (self.selected_album.clone(), self.list_state.selected())
                {
                    self.events.push_back(CenterPanelEvent::PlaylistTrackMoved {
                        path,
                        from: index,
                        to: index.saturating_sub(1),
                    });
                }
            }
            _ => {}
        }
    }

    fn handle_album_results(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_album_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_album_item(),
            KeyCode::Home | KeyCode::Char('g') => {
                Self::select_first(&mut self.album_list_state, self.album_display_titles.len())
            }
            KeyCode::End | KeyCode::Char('G') => {
                Self::select_last(&mut self.album_list_state, self.album_display_titles.len())
            }
            KeyCode::PageDown => {
                Self::select_next_page(&mut self.album_list_state, self.album_display_titles.len())
            }
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.album_list_state,
                self.album_display_titles.len(),
            ),
            KeyCode::Char('/') => self.open_search(),
            KeyCode::Enter => {
                if let Some(index) = self.album_list_state.selected() {
                    self.events
                        .push_back(CenterPanelEvent::AlbumSelected(index));
                }
            }
            KeyCode::Esc => {
                self.album_display_titles.clear();
                self.album_list_state.select(None);
                self.mode = CenterPanelMode::Album;
            }
            _ => {}
        }
    }

    fn handle_album_tracks(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            KeyCode::Home | KeyCode::Char('g') => {
                Self::select_first(&mut self.list_state, self.songs.len())
            }
            KeyCode::End | KeyCode::Char('G') => {
                Self::select_last(&mut self.list_state, self.songs.len())
            }
            KeyCode::PageDown => Self::select_next_page(&mut self.list_state, self.songs.len()),
            KeyCode::PageUp => Self::select_previous_page(&mut self.list_state, self.songs.len()),
            KeyCode::Enter => {
                if self.list_state.selected().is_some() {
                    self.events.push_back(CenterPanelEvent::SongSelected);
                }
            }
            KeyCode::Esc => {
                // Go back to album results
                self.songs.clear();
                self.list_state.select(None);
                self.viewing_album_title = None;
                self.mode = CenterPanelMode::AlbumResults;
            }
            _ => {}
        }
    }

    fn handle_artist_results(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_artist_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_artist_item(),
            KeyCode::Home | KeyCode::Char('g') => Self::select_first(
                &mut self.artist_list_state,
                self.artist_display_titles.len(),
            ),
            KeyCode::End | KeyCode::Char('G') => Self::select_last(
                &mut self.artist_list_state,
                self.artist_display_titles.len(),
            ),
            KeyCode::PageDown => Self::select_next_page(
                &mut self.artist_list_state,
                self.artist_display_titles.len(),
            ),
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.artist_list_state,
                self.artist_display_titles.len(),
            ),
            KeyCode::Char('/') => self.open_search(),
            KeyCode::Enter => {
                if let Some(index) = self.artist_list_state.selected() {
                    self.events
                        .push_back(CenterPanelEvent::ArtistSelected(index));
                }
            }
            KeyCode::Esc => {
                self.artist_display_titles.clear();
                self.artist_list_state.select(None);
                self.mode = CenterPanelMode::Album;
            }
            _ => {}
        }
    }

    fn render_create_playlist(&mut self, frame: &mut Frame, area: Rect, _is_focused: bool) {
        let title = match &self.playlist_status_message {
            Some(message) => format!(" New Playlist Name ({}) ", message),
            None => " New Playlist Name ".to_string(),
        };
        let input_block = Block::bordered()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::accent_style());

        let mut input_spans = vec![Span::styled("> ", theme::accent_bold_style())];
        input_spans.extend(
            self.playlist_name_input
                .display_spans(true, theme::accent_style()),
        );
        let input_text = Line::from(input_spans);

        let paragraph = Paragraph::new(input_text).block(input_block);
        frame.render_widget(paragraph, area);
    }

    fn render_rename_playlist(&mut self, frame: &mut Frame, area: Rect, _is_focused: bool) {
        let title = match &self.playlist_status_message {
            Some(message) => format!(" Rename Playlist ({}) ", message),
            None => " Rename Playlist ".to_string(),
        };
        let input_block = Block::bordered()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::accent_style());

        let mut input_spans = vec![Span::styled("> ", theme::accent_bold_style())];
        input_spans.extend(
            self.playlist_name_input
                .display_spans(true, theme::accent_style()),
        );
        let input_text = Line::from(input_spans);

        let paragraph = Paragraph::new(input_text).block(input_block);
        frame.render_widget(paragraph, area);
    }

    fn render_duplicate_playlist(&mut self, frame: &mut Frame, area: Rect, _is_focused: bool) {
        let title = match &self.playlist_status_message {
            Some(message) => format!(" Duplicate Playlist ({}) ", message),
            None => " Duplicate Playlist ".to_string(),
        };
        let input_block = Block::bordered()
            .title(title)
            .borders(Borders::ALL)
            .border_style(theme::accent_style());

        let mut input_spans = vec![Span::styled("> ", theme::accent_bold_style())];
        input_spans.extend(
            self.playlist_name_input
                .display_spans(true, theme::accent_style()),
        );
        let input_text = Line::from(input_spans);

        let paragraph = Paragraph::new(input_text).block(input_block);
        frame.render_widget(paragraph, area);
    }

    fn render_playlist_picker(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);

        let list_items: Vec<ListItem> = self
            .playlist_picker_names
            .iter()
            .map(|n| ListItem::new(n.as_str()))
            .collect();

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(" Add to Playlist (Enter to select, Esc to cancel) ")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(list, area, &mut self.playlist_picker_state);
    }

    fn handle_create_playlist(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.playlist_status_message = None;
                self.playlist_name_input.exit_input_mode();
                self.events
                    .push_back(CenterPanelEvent::PlaylistAddCancelled);
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            KeyCode::Enter => {
                let name = self.playlist_name_input.value.trim().to_string();
                if name.is_empty() {
                    self.playlist_status_message = Some("Name required".to_string());
                } else {
                    self.events
                        .push_back(CenterPanelEvent::PlaylistCreated(name));
                }
            }
            KeyCode::Char(c) => {
                self.playlist_status_message = None;
                self.playlist_name_input.append_char(c);
            }
            KeyCode::Backspace => {
                self.playlist_status_message = None;
                self.playlist_name_input.delete_char();
            }
            KeyCode::Delete => {
                self.playlist_status_message = None;
                self.playlist_name_input.delete_next_char();
            }
            KeyCode::Left => self.playlist_name_input.move_cursor_left(),
            KeyCode::Right => self.playlist_name_input.move_cursor_right(),
            KeyCode::Home => self.playlist_name_input.move_cursor_to_start(),
            KeyCode::End => self.playlist_name_input.move_cursor_to_end(),
            _ => {}
        }
    }

    fn handle_rename_playlist(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.playlist_status_message = None;
                self.playlist_name_input.exit_input_mode();
                self.events
                    .push_back(CenterPanelEvent::PlaylistRenameCancelled);
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            KeyCode::Enter => {
                let name = self.playlist_name_input.value.trim().to_string();
                if name.is_empty() {
                    self.playlist_status_message = Some("Name required".to_string());
                } else {
                    self.events
                        .push_back(CenterPanelEvent::PlaylistRenamed(name));
                }
            }
            KeyCode::Char(c) => {
                self.playlist_status_message = None;
                self.playlist_name_input.append_char(c);
            }
            KeyCode::Backspace => {
                self.playlist_status_message = None;
                self.playlist_name_input.delete_char();
            }
            KeyCode::Delete => {
                self.playlist_status_message = None;
                self.playlist_name_input.delete_next_char();
            }
            KeyCode::Left => self.playlist_name_input.move_cursor_left(),
            KeyCode::Right => self.playlist_name_input.move_cursor_right(),
            KeyCode::Home => self.playlist_name_input.move_cursor_to_start(),
            KeyCode::End => self.playlist_name_input.move_cursor_to_end(),
            _ => {}
        }
    }

    fn handle_duplicate_playlist(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.playlist_status_message = None;
                self.playlist_name_input.exit_input_mode();
                self.events
                    .push_back(CenterPanelEvent::PlaylistDuplicateCancelled);
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            KeyCode::Enter => {
                let name = self.playlist_name_input.value.trim().to_string();
                if name.is_empty() {
                    self.playlist_status_message = Some("Name required".to_string());
                } else {
                    self.events
                        .push_back(CenterPanelEvent::PlaylistDuplicated(name));
                }
            }
            KeyCode::Char(c) => {
                self.playlist_status_message = None;
                self.playlist_name_input.append_char(c);
            }
            KeyCode::Backspace => {
                self.playlist_status_message = None;
                self.playlist_name_input.delete_char();
            }
            KeyCode::Delete => {
                self.playlist_status_message = None;
                self.playlist_name_input.delete_next_char();
            }
            KeyCode::Left => self.playlist_name_input.move_cursor_left(),
            KeyCode::Right => self.playlist_name_input.move_cursor_right(),
            KeyCode::Home => self.playlist_name_input.move_cursor_to_start(),
            KeyCode::End => self.playlist_name_input.move_cursor_to_end(),
            _ => {}
        }
    }

    fn handle_playlist_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.playlist_picker_names.is_empty() {
                    return;
                }
                let i = match self.playlist_picker_state.selected() {
                    Some(i) => {
                        if i >= self.playlist_picker_names.len() - 1 {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.playlist_picker_state.select(Some(i));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.playlist_picker_names.is_empty() {
                    return;
                }
                let i = match self.playlist_picker_state.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.playlist_picker_names.len() - 1
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.playlist_picker_state.select(Some(i));
            }
            KeyCode::Home | KeyCode::Char('g') => Self::select_first(
                &mut self.playlist_picker_state,
                self.playlist_picker_names.len(),
            ),
            KeyCode::End | KeyCode::Char('G') => Self::select_last(
                &mut self.playlist_picker_state,
                self.playlist_picker_names.len(),
            ),
            KeyCode::PageDown => Self::select_next_page(
                &mut self.playlist_picker_state,
                self.playlist_picker_names.len(),
            ),
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.playlist_picker_state,
                self.playlist_picker_names.len(),
            ),
            KeyCode::Enter => {
                if let Some(index) = self.playlist_picker_state.selected() {
                    self.events
                        .push_back(CenterPanelEvent::PlaylistSelectedForAdd(index));
                }
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            KeyCode::Esc => {
                self.events
                    .push_back(CenterPanelEvent::PlaylistAddCancelled);
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            _ => {}
        }
    }

    fn handle_queue(&mut self, key: KeyEvent) {
        if self.queue_filter_input.is_input_mode() {
            self.handle_queue_filter_input(key);
            return;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_queue_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_queue_item(),
            KeyCode::Home | KeyCode::Char('g') => {
                Self::select_first(&mut self.queue_list_state, self.queue_visible_indices.len())
            }
            KeyCode::End | KeyCode::Char('G') => {
                Self::select_last(&mut self.queue_list_state, self.queue_visible_indices.len())
            }
            KeyCode::PageDown => {
                Self::select_next_page(&mut self.queue_list_state, self.queue_visible_indices.len())
            }
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.queue_list_state,
                self.queue_visible_indices.len(),
            ),
            KeyCode::Char('f') => self.open_queue_filter(),
            KeyCode::Char('J') => {
                if let Some(index) = self.selected_queue_index() {
                    self.events.push_back(CenterPanelEvent::QueueItemMoved {
                        from: index,
                        to: index.saturating_add(1),
                    });
                }
            }
            KeyCode::Char('K') => {
                if let Some(index) = self.selected_queue_index() {
                    self.events.push_back(CenterPanelEvent::QueueItemMoved {
                        from: index,
                        to: index.saturating_sub(1),
                    });
                }
            }
            KeyCode::Char('d') => {
                if let Some(index) = self.selected_queue_index() {
                    if index == self.queue_position {
                        self.events
                            .push_back(CenterPanelEvent::QueueCurrentItemRemovalBlocked);
                    } else {
                        self.events
                            .push_back(CenterPanelEvent::QueueItemRemoved(index));
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(index) = self.selected_queue_index() {
                    self.events
                        .push_back(CenterPanelEvent::QueueItemJumped(index));
                }
            }
            KeyCode::Char('c') => {
                self.events.push_back(CenterPanelEvent::QueueClearRequested);
            }
            KeyCode::Char('S') => {
                self.events.push_back(CenterPanelEvent::QueueSaveRequested);
            }
            KeyCode::Esc => {
                if self.has_queue_filter_query() {
                    self.clear_queue_filter();
                } else {
                    self.mode = self.pre_queue_mode.take().unwrap_or(CenterPanelMode::Album);
                }
            }
            _ => {}
        }
    }

    fn handle_queue_filter_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.clear_queue_filter(),
            KeyCode::Enter => self.queue_filter_input.confirm_input(),
            KeyCode::Char(c) => {
                let selected_original_index = self.selected_queue_index();
                self.queue_filter_input.append_char(c);
                self.refresh_queue_visible_indices(selected_original_index);
            }
            KeyCode::Backspace => {
                let selected_original_index = self.selected_queue_index();
                self.queue_filter_input.delete_char();
                self.refresh_queue_visible_indices(selected_original_index);
            }
            KeyCode::Delete => {
                let selected_original_index = self.selected_queue_index();
                self.queue_filter_input.delete_next_char();
                self.refresh_queue_visible_indices(selected_original_index);
            }
            KeyCode::Left => self.queue_filter_input.move_cursor_left(),
            KeyCode::Right => self.queue_filter_input.move_cursor_right(),
            KeyCode::Home => self.queue_filter_input.move_cursor_to_start(),
            KeyCode::End => self.queue_filter_input.move_cursor_to_end(),
            KeyCode::Down => self.next_queue_item(),
            KeyCode::Up => self.previous_queue_item(),
            KeyCode::PageDown => {
                Self::select_next_page(&mut self.queue_list_state, self.queue_visible_indices.len())
            }
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.queue_list_state,
                self.queue_visible_indices.len(),
            ),
            _ => {}
        }
    }

    fn handle_history(&mut self, key: KeyEvent) {
        if self.history_filter_input.is_input_mode() {
            self.handle_history_filter_input(key);
            return;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_history_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_history_item(),
            KeyCode::Home | KeyCode::Char('g') => Self::select_first(
                &mut self.history_list_state,
                self.history_visible_indices.len(),
            ),
            KeyCode::End | KeyCode::Char('G') => Self::select_last(
                &mut self.history_list_state,
                self.history_visible_indices.len(),
            ),
            KeyCode::PageDown => Self::select_next_page(
                &mut self.history_list_state,
                self.history_visible_indices.len(),
            ),
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.history_list_state,
                self.history_visible_indices.len(),
            ),
            KeyCode::Enter => {
                if self.selected_history_index().is_some() {
                    self.events.push_back(CenterPanelEvent::SongSelected);
                }
            }
            KeyCode::Char('f') => self.open_history_filter(),
            KeyCode::Char('d') => {
                if let Some(index) = self.selected_history_index() {
                    self.events
                        .push_back(CenterPanelEvent::HistoryItemRemoved(index));
                }
            }
            KeyCode::Char('c') => {
                self.events
                    .push_back(CenterPanelEvent::HistoryClearRequested);
            }
            KeyCode::Esc => {
                if self.has_history_filter_query() {
                    self.clear_history_filter();
                } else {
                    self.mode = self
                        .pre_history_mode
                        .take()
                        .unwrap_or(CenterPanelMode::Album);
                }
            }
            _ => {}
        }
    }

    fn handle_history_filter_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.clear_history_filter(),
            KeyCode::Enter => self.history_filter_input.confirm_input(),
            KeyCode::Char(c) => {
                let selected_original_index = self.selected_history_index();
                self.history_filter_input.append_char(c);
                self.refresh_history_visible_indices(selected_original_index);
            }
            KeyCode::Backspace => {
                let selected_original_index = self.selected_history_index();
                self.history_filter_input.delete_char();
                self.refresh_history_visible_indices(selected_original_index);
            }
            KeyCode::Delete => {
                let selected_original_index = self.selected_history_index();
                self.history_filter_input.delete_next_char();
                self.refresh_history_visible_indices(selected_original_index);
            }
            KeyCode::Left => self.history_filter_input.move_cursor_left(),
            KeyCode::Right => self.history_filter_input.move_cursor_right(),
            KeyCode::Home => self.history_filter_input.move_cursor_to_start(),
            KeyCode::End => self.history_filter_input.move_cursor_to_end(),
            KeyCode::Down => self.next_history_item(),
            KeyCode::Up => self.previous_history_item(),
            KeyCode::PageDown => Self::select_next_page(
                &mut self.history_list_state,
                self.history_visible_indices.len(),
            ),
            KeyCode::PageUp => Self::select_previous_page(
                &mut self.history_list_state,
                self.history_visible_indices.len(),
            ),
            _ => {}
        }
    }

    pub fn get_selected_song(&mut self) -> Option<Song> {
        if let Some(index) = self.list_state.selected() {
            Some(self.songs[index].clone())
        } else {
            None
        }
    }

    pub fn get_selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub fn select_song_index(&mut self, index: usize) {
        if index < self.songs.len() {
            self.list_state.select(Some(index));
        }
    }

    pub fn get_songs(&self) -> Vec<Song> {
        self.songs.clone()
    }

    pub fn can_open_local_filter(&self) -> bool {
        !self.songs.is_empty() || !self.album_songs.is_empty()
    }

    pub fn selected_songs_for_playlist(&self) -> Vec<Song> {
        match self.mode {
            CenterPanelMode::Album
            | CenterPanelMode::SearchResults
            | CenterPanelMode::AlbumTracks => self
                .list_state
                .selected()
                .and_then(|index| self.songs.get(index))
                .cloned()
                .into_iter()
                .collect(),
            CenterPanelMode::History => self
                .selected_history_index()
                .and_then(|index| self.history_songs.get(index))
                .cloned()
                .into_iter()
                .collect(),
            CenterPanelMode::Queue => self
                .selected_queue_index()
                .and_then(|index| self.queue_songs.get(index))
                .cloned()
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn next_item(&mut self) {
        if self.songs.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.songs.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn previous_item(&mut self) {
        if self.songs.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.songs.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn next_history_item(&mut self) {
        if self.history_visible_indices.is_empty() {
            return;
        }
        let i = match self.history_list_state.selected() {
            Some(i) => {
                if i >= self.history_visible_indices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.history_list_state.select(Some(i));
    }

    fn previous_history_item(&mut self) {
        if self.history_visible_indices.is_empty() {
            return;
        }
        let i = match self.history_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.history_visible_indices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.history_list_state.select(Some(i));
    }

    fn select_first(list_state: &mut ListState, len: usize) {
        if len > 0 {
            list_state.select(Some(0));
        }
    }

    fn select_last(list_state: &mut ListState, len: usize) {
        if len > 0 {
            list_state.select(Some(len - 1));
        }
    }

    fn select_next_page(list_state: &mut ListState, len: usize) {
        if len > 0 {
            let current = list_state.selected().unwrap_or(0);
            list_state.select(Some(current.saturating_add(PAGE_STEP).min(len - 1)));
        }
    }

    fn select_previous_page(list_state: &mut ListState, len: usize) {
        if len > 0 {
            let current = list_state.selected().unwrap_or(0);
            list_state.select(Some(current.saturating_sub(PAGE_STEP)));
        }
    }

    fn next_album_item(&mut self) {
        if self.album_display_titles.is_empty() {
            return;
        }
        let i = match self.album_list_state.selected() {
            Some(i) => {
                if i >= self.album_display_titles.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.album_list_state.select(Some(i));
    }

    fn previous_album_item(&mut self) {
        if self.album_display_titles.is_empty() {
            return;
        }
        let i = match self.album_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.album_display_titles.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.album_list_state.select(Some(i));
    }

    fn next_queue_item(&mut self) {
        if self.queue_visible_indices.is_empty() {
            return;
        }
        let i = match self.queue_list_state.selected() {
            Some(i) => {
                if i >= self.queue_visible_indices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.queue_list_state.select(Some(i));
    }

    fn previous_queue_item(&mut self) {
        if self.queue_visible_indices.is_empty() {
            return;
        }
        let i = match self.queue_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.queue_visible_indices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.queue_list_state.select(Some(i));
    }

    fn next_artist_item(&mut self) {
        if self.artist_display_titles.is_empty() {
            return;
        }
        let i = match self.artist_list_state.selected() {
            Some(i) => {
                if i >= self.artist_display_titles.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.artist_list_state.select(Some(i));
    }

    fn previous_artist_item(&mut self) {
        if self.artist_display_titles.is_empty() {
            return;
        }
        let i = match self.artist_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.artist_display_titles.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.artist_list_state.select(Some(i));
    }
}

impl Default for CenterPanel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str) -> Song {
        Song {
            title: title.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn queue_sync_preserves_user_selection_when_still_valid() {
        let mut panel = CenterPanel::new();
        let queue = vec![song("First"), song("Second"), song("Third")];
        panel.set_queue(queue.clone(), 0);
        panel.show_queue();
        panel.handle_events(KeyEvent::from(KeyCode::Char('j')));

        assert_eq!(panel.selected_songs_for_playlist()[0].title, "Second");

        panel.set_queue(queue, 0);

        assert_eq!(panel.selected_songs_for_playlist()[0].title, "Second");
    }

    #[test]
    fn queue_filter_maps_visible_selection_to_original_queue_index() {
        let mut panel = CenterPanel::new();
        panel.set_queue(vec![song("First"), song("Second"), song("Third")], 0);
        panel.show_queue();

        panel.handle_events(KeyEvent::from(KeyCode::Char('f')));
        for key in "third".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }

        assert_eq!(panel.queue_visible_indices, vec![2]);
        assert_eq!(panel.selected_queue_index(), Some(2));
        assert_eq!(panel.selected_songs_for_playlist()[0].title, "Third");

        panel.handle_events(KeyEvent::from(KeyCode::Enter));
        panel.handle_events(KeyEvent::from(KeyCode::Enter));

        assert_eq!(
            panel.next_event(),
            Some(CenterPanelEvent::QueueItemJumped(2))
        );
    }

    #[test]
    fn queue_filter_escape_clears_query_before_closing_queue() {
        let mut panel = CenterPanel::new();
        panel.set_queue(vec![song("First"), song("Second"), song("Third")], 0);
        panel.show_queue();

        panel.handle_events(KeyEvent::from(KeyCode::Char('f')));
        for key in "third".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }
        panel.handle_events(KeyEvent::from(KeyCode::Esc));

        assert_eq!(panel.mode, CenterPanelMode::Queue);
        assert!(!panel.has_queue_filter_query());
        assert_eq!(panel.queue_visible_indices, vec![0, 1, 2]);

        panel.handle_events(KeyEvent::from(KeyCode::Esc));

        assert_eq!(panel.mode, CenterPanelMode::Album);
    }

    #[test]
    fn history_filter_maps_visible_selection_to_original_history_index() {
        let mut panel = CenterPanel::new();
        panel.set_history(vec![song("First"), song("Second"), song("Third")]);
        panel.show_history();

        panel.handle_events(KeyEvent::from(KeyCode::Char('f')));
        for key in "third".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }

        assert_eq!(panel.history_visible_indices, vec![2]);
        assert_eq!(panel.selected_history_index(), Some(2));
        assert_eq!(panel.selected_songs_for_playlist()[0].title, "Third");

        panel.handle_events(KeyEvent::from(KeyCode::Enter));
        panel.handle_events(KeyEvent::from(KeyCode::Char('d')));

        assert_eq!(
            panel.next_event(),
            Some(CenterPanelEvent::HistoryItemRemoved(2))
        );
    }

    #[test]
    fn history_filter_escape_clears_query_before_closing_history() {
        let mut panel = CenterPanel::new();
        panel.set_history(vec![song("First"), song("Second"), song("Third")]);
        panel.show_history();

        panel.handle_events(KeyEvent::from(KeyCode::Char('f')));
        for key in "third".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }
        panel.handle_events(KeyEvent::from(KeyCode::Esc));

        assert_eq!(panel.mode, CenterPanelMode::History);
        assert!(!panel.has_history_filter_query());
        assert_eq!(panel.history_visible_indices, vec![0, 1, 2]);

        panel.handle_events(KeyEvent::from(KeyCode::Esc));

        assert_eq!(panel.mode, CenterPanelMode::Album);
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_song_rows() {
        let mut panel = CenterPanel::new();
        panel.set_album(
            PathBuf::from("/music/album"),
            vec![song("First"), song("Second"), song("Third")],
        );

        panel.handle_events(KeyEvent::from(KeyCode::End));
        assert_eq!(panel.get_selected_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        assert_eq!(panel.get_selected_index(), Some(0));

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.get_selected_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.get_selected_index(), Some(0));
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_result_rows() {
        let mut panel = CenterPanel::new();
        panel.set_album_results(vec![
            "First Album".to_string(),
            "Second Album".to_string(),
            "Third Album".to_string(),
        ]);

        panel.handle_events(KeyEvent::from(KeyCode::End));
        assert_eq!(panel.album_list_state.selected(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        assert_eq!(panel.album_list_state.selected(), Some(0));

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.album_list_state.selected(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.album_list_state.selected(), Some(0));

        panel.set_artist_results(vec![
            "First Artist".to_string(),
            "Second Artist".to_string(),
            "Third Artist".to_string(),
        ]);

        panel.handle_events(KeyEvent::from(KeyCode::End));
        assert_eq!(panel.artist_list_state.selected(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        assert_eq!(panel.artist_list_state.selected(), Some(0));

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.artist_list_state.selected(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.artist_list_state.selected(), Some(0));
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_queue_rows() {
        let mut panel = CenterPanel::new();
        panel.set_queue(vec![song("First"), song("Second"), song("Third")], 0);
        panel.show_queue();

        panel.handle_events(KeyEvent::from(KeyCode::End));
        assert_eq!(panel.selected_queue_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        assert_eq!(panel.selected_queue_index(), Some(0));

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.selected_queue_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.selected_queue_index(), Some(0));
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_history_rows() {
        let mut panel = CenterPanel::new();
        panel.set_history(vec![song("First"), song("Second"), song("Third")]);
        panel.show_history();

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.selected_history_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.selected_history_index(), Some(0));
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_playlist_picker_rows() {
        let mut panel = CenterPanel::new();
        panel.open_playlist_picker(vec![
            "First".to_string(),
            "Second".to_string(),
            "Third".to_string(),
        ]);

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.playlist_picker_state.selected(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.playlist_picker_state.selected(), Some(0));
    }

    #[test]
    fn page_up_and_page_down_move_song_selection_by_page() {
        let mut panel = CenterPanel::new();
        let songs: Vec<Song> = (1..=12).map(|n| song(&format!("Song {n}"))).collect();
        panel.set_album(PathBuf::from("/music/album"), songs);

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.get_selected_index(), Some(10));

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.get_selected_index(), Some(11));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.get_selected_index(), Some(1));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.get_selected_index(), Some(0));
    }

    #[test]
    fn page_up_and_page_down_move_queue_selection_by_page() {
        let mut panel = CenterPanel::new();
        let songs: Vec<Song> = (1..=12).map(|n| song(&format!("Song {n}"))).collect();
        panel.set_queue(songs, 0);
        panel.show_queue();

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.selected_queue_index(), Some(10));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.selected_queue_index(), Some(0));
    }

    #[test]
    fn queueable_collection_requires_an_open_album_or_album_tracks_view() {
        let mut panel = CenterPanel::new();

        assert!(!panel.is_showing_queueable_collection());

        panel.set_album(PathBuf::from("/music/album"), Vec::new());
        assert!(panel.is_showing_queueable_collection());

        let mut panel = CenterPanel::new();
        panel.set_album_tracks("Stream Album".to_string(), Vec::new());
        assert!(panel.is_showing_queueable_collection());
    }

    #[test]
    fn opening_streaming_search_clears_stale_results_and_status() {
        let mut panel = CenterPanel::new();
        panel.set_artist_results(vec!["Old Artist".to_string()]);
        panel.set_album_results(vec!["Old Album".to_string()]);
        panel.set_search_results(vec![song("Old Track")]);
        panel.set_status(Some("Search timed out".to_string()));

        panel.open_search();

        assert!(panel.artist_display_titles.is_empty());
        assert!(panel.album_display_titles.is_empty());
        assert!(panel.songs.is_empty());
        assert!(panel.status_message.is_none());
        assert_eq!(panel.mode, CenterPanelMode::SearchInput);
    }

    #[test]
    fn opening_local_search_clears_stale_status_but_keeps_album_songs() {
        let mut panel = CenterPanel::new();
        panel.set_album(
            PathBuf::from("/music/album"),
            vec![song("First"), song("Second")],
        );
        panel.set_status(Some("Search timed out".to_string()));

        panel.open_search_local();

        assert_eq!(panel.songs.len(), 2);
        assert_eq!(panel.album_songs.len(), 2);
        assert!(panel.status_message.is_none());
        assert_eq!(panel.mode, CenterPanelMode::SearchInput);
    }

    #[test]
    fn blank_playlist_name_shows_inline_feedback() {
        let mut panel = CenterPanel::new();
        panel.open_create_playlist();

        panel.handle_events(KeyEvent::from(KeyCode::Enter));

        assert_eq!(panel.next_event(), None);
        assert_eq!(panel.mode, CenterPanelMode::CreatePlaylist);
        assert_eq!(
            panel.playlist_status_message.as_deref(),
            Some("Name required")
        );
    }

    #[test]
    fn blank_search_query_shows_feedback_and_typing_clears_it() {
        let mut panel = CenterPanel::new();
        panel.open_search();

        panel.handle_events(KeyEvent::from(KeyCode::Enter));

        assert_eq!(panel.next_event(), None);
        assert_eq!(panel.mode, CenterPanelMode::SearchInput);
        assert_eq!(panel.status_message.as_deref(), Some("Query required"));

        panel.handle_events(KeyEvent::from(KeyCode::Char('a')));

        assert!(panel.status_message.is_none());
    }

    #[test]
    fn search_input_supports_home_end_and_delete_before_submit() {
        let mut panel = CenterPanel::new();
        panel.open_search();

        for key in "Xalbum".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }
        panel.handle_events(KeyEvent::from(KeyCode::Home));
        panel.handle_events(KeyEvent::from(KeyCode::Delete));
        panel.handle_events(KeyEvent::from(KeyCode::End));
        panel.handle_events(KeyEvent::from(KeyCode::Char('s')));
        panel.handle_events(KeyEvent::from(KeyCode::Enter));

        assert_eq!(
            panel.next_event(),
            Some(CenterPanelEvent::QuerySubmitted("albums".to_string()))
        );
    }

    #[test]
    fn blank_local_filter_submits_empty_query() {
        let mut panel = CenterPanel::new();
        panel.set_album(
            PathBuf::from("/music/album"),
            vec![song("First"), song("Second")],
        );
        panel.open_search_local();

        panel.handle_events(KeyEvent::from(KeyCode::Enter));

        assert_eq!(
            panel.next_event(),
            Some(CenterPanelEvent::QuerySubmitted(String::new()))
        );
        assert_eq!(panel.mode, CenterPanelMode::SearchResults);
        assert!(panel.status_message.is_none());
    }

    #[test]
    fn local_filter_recomputes_after_delete_key() {
        let mut panel = CenterPanel::new();
        panel.set_album(
            PathBuf::from("/music/album"),
            vec![song("First"), song("Second")],
        );
        panel.open_search_local();

        for key in "XSecond".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }
        assert!(panel.songs.is_empty());

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        panel.handle_events(KeyEvent::from(KeyCode::Delete));

        assert_eq!(
            panel
                .songs
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Second"]
        );
    }

    #[test]
    fn local_filter_updates_visible_songs_while_typing() {
        let mut panel = CenterPanel::new();
        panel.set_album(
            PathBuf::from("/music/album"),
            vec![song("First"), song("Second")],
        );
        panel.open_search_local();

        for key in ['s', 'e', 'c'] {
            panel.handle_events(KeyEvent::from(KeyCode::Char(key)));
        }

        assert_eq!(panel.mode, CenterPanelMode::SearchInput);
        assert_eq!(
            panel
                .songs
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Second"]
        );

        for _ in 0..3 {
            panel.handle_events(KeyEvent::from(KeyCode::Backspace));
        }

        assert_eq!(panel.mode, CenterPanelMode::SearchInput);
        assert_eq!(
            panel
                .songs
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second"]
        );
    }

    #[test]
    fn song_row_label_includes_artist_and_duration_when_available() {
        let song = Song {
            title: "Ceremony".to_string(),
            artist: "New Order".to_string(),
            duration_secs: Some(287.0),
            ..Default::default()
        };

        assert_eq!(
            CenterPanel::song_row_label(&song),
            "New Order - Ceremony (4:47)"
        );
    }

    #[test]
    fn song_row_label_includes_album_context_when_available() {
        let song = Song {
            title: "Ceremony".to_string(),
            artist: "New Order".to_string(),
            album_name: "Substance".to_string(),
            duration_secs: Some(287.0),
            ..Default::default()
        };

        assert_eq!(
            CenterPanel::song_row_label(&song),
            "New Order - Ceremony (4:47) [Substance]"
        );
    }

    #[test]
    fn numbered_song_row_label_includes_disc_and_track_metadata() {
        let song = Song {
            title: "Second Movement".to_string(),
            artist: "Performer".to_string(),
            disc_number: Some(2),
            track_number: Some(4),
            duration_secs: Some(185.0),
            ..Default::default()
        };

        assert_eq!(
            CenterPanel::numbered_song_row_label(0, &song),
            "2.04 Performer - Second Movement (3:05)"
        );
    }

    #[test]
    fn queue_song_row_label_includes_position_and_current_marker() {
        let song = Song {
            title: "Second Song".to_string(),
            artist: "Second Artist".to_string(),
            ..Default::default()
        };

        assert_eq!(
            CenterPanel::queue_song_row_label(1, 12, &song, true),
            ">  2. Second Artist - Second Song"
        );
        assert_eq!(
            CenterPanel::queue_song_row_label(9, 12, &song, false),
            "  10. Second Artist - Second Song"
        );
    }

    #[test]
    fn current_song_identity_matches_local_paths() {
        let listed = Song {
            title: "Listed".to_string(),
            path: PathBuf::from("/music/track.flac"),
            ..Default::default()
        };
        let current = Song {
            title: "Current".to_string(),
            path: PathBuf::from("/music/track.flac"),
            ..Default::default()
        };
        let other = Song {
            title: "Other".to_string(),
            path: PathBuf::from("/music/other.flac"),
            ..Default::default()
        };

        assert!(CenterPanel::song_identity_matches(&listed, &current));
        assert!(!CenterPanel::song_identity_matches(&listed, &other));
    }

    #[test]
    fn current_song_identity_matches_stream_references() {
        let listed = Song {
            title: "Listed".to_string(),
            stream_service: Some("Qobuz".to_string()),
            stream_track_id: Some("track-1".to_string()),
            ..Default::default()
        };
        let current = Song {
            title: "Current".to_string(),
            stream_service: Some("qobuz".to_string()),
            stream_track_id: Some("track-1".to_string()),
            url: Some("https://cdn.example/stream.flac".to_string()),
            ..Default::default()
        };
        let other = Song {
            title: "Other".to_string(),
            stream_service: Some("Tidal".to_string()),
            stream_track_id: Some("track-1".to_string()),
            ..Default::default()
        };

        assert!(CenterPanel::song_identity_matches(&listed, &current));
        assert!(!CenterPanel::song_identity_matches(&listed, &other));
    }

    #[test]
    fn song_row_label_omits_empty_metadata() {
        let song = Song {
            title: "Untitled.flac".to_string(),
            duration_secs: Some(0.0),
            ..Default::default()
        };

        assert_eq!(CenterPanel::song_row_label(&song), "Untitled.flac");
    }

    #[test]
    fn collection_title_appends_total_duration_when_available() {
        let songs = vec![
            Song {
                title: "First".to_string(),
                duration_secs: Some(185.0),
                ..Default::default()
            },
            Song {
                title: "Second".to_string(),
                duration_secs: Some(190.0),
                ..Default::default()
            },
        ];

        assert_eq!(
            CenterPanel::collection_title("Queue (2 tracks)", &songs),
            "Queue (2 tracks) - 6:15"
        );
    }

    #[test]
    fn collection_title_omits_total_duration_when_metadata_is_missing() {
        let songs = vec![
            Song {
                title: "Unknown Duration".to_string(),
                ..Default::default()
            },
            Song {
                title: "Invalid Duration".to_string(),
                duration_secs: Some(0.0),
                ..Default::default()
            },
        ];

        assert_eq!(
            CenterPanel::collection_title("Queue (2 tracks)", &songs),
            "Queue (2 tracks)"
        );
    }
}
