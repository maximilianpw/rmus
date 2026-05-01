use std::{collections::VecDeque, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{sources::song::Song, ui::input_line::InputLine};

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
    ArtistResults,
    CreatePlaylist,
    PlaylistPicker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CenterPanelEvent {
    QuerySubmitted(String),
    AlbumSelected(usize),
    ArtistSelected(usize),
    QueueItemRemoved(usize),
    QueueItemJumped(usize),
    PlaylistCreated(String),
    PlaylistSelectedForAdd(usize),
}

#[derive(Debug)]
pub struct CenterPanel {
    selected_album: Option<PathBuf>,
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
    /// The mode to return to when closing the queue view.
    pre_queue_mode: Option<CenterPanelMode>,
    /// Current search mode (Albums/Artists/Tracks).
    search_mode: SearchMode,
    /// Artist display titles for ArtistResults mode.
    artist_display_titles: Vec<String>,
    artist_list_state: ListState,
    /// Input for creating a new playlist name.
    playlist_name_input: InputLine,
    /// Playlist names for the picker overlay.
    playlist_picker_names: Vec<String>,
    playlist_picker_state: ListState,
    /// The mode to return to when closing playlist create/picker.
    pre_playlist_mode: Option<CenterPanelMode>,
}

impl CenterPanel {
    pub fn new() -> Self {
        Self {
            selected_album: None,
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
            pre_queue_mode: None,
            search_mode: SearchMode::default(),
            artist_display_titles: Vec::new(),
            artist_list_state: ListState::default(),
            playlist_name_input: InputLine::new(),
            playlist_picker_names: Vec::new(),
            playlist_picker_state: ListState::default(),
            pre_playlist_mode: None,
        }
    }

    pub fn set_status(&mut self, message: Option<String>) {
        self.status_message = message;
    }

    pub fn set_album(&mut self, path: PathBuf, songs: Vec<Song>) {
        self.selected_album = Some(path);
        self.album_songs = songs.clone();
        self.songs = songs;
        self.mode = CenterPanelMode::Album;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn set_search_results(&mut self, songs: Vec<Song>) {
        self.songs = songs;
        self.mode = CenterPanelMode::SearchResults;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn set_album_results(&mut self, display_titles: Vec<String>) {
        self.album_display_titles = display_titles;
        self.mode = CenterPanelMode::AlbumResults;
        if !self.album_display_titles.is_empty() {
            self.album_list_state.select(Some(0));
        } else {
            self.album_list_state.select(None);
        }
    }

    pub fn set_album_tracks(&mut self, album_title: String, songs: Vec<Song>) {
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
        self.queue_songs = songs;
        self.queue_position = position;
        if !self.queue_songs.is_empty() {
            self.queue_list_state.select(Some(position));
        } else {
            self.queue_list_state.select(None);
        }
    }

    pub fn show_queue(&mut self) {
        if self.mode != CenterPanelMode::Queue {
            self.pre_queue_mode = Some(self.mode);
        }
        self.mode = CenterPanelMode::Queue;
        if !self.queue_songs.is_empty() && self.queue_list_state.selected().is_none() {
            self.queue_list_state.select(Some(self.queue_position));
        }
    }

    pub fn is_showing_queue(&self) -> bool {
        self.mode == CenterPanelMode::Queue
    }

    pub fn search_mode(&self) -> SearchMode {
        self.search_mode
    }

    pub fn set_artist_results(&mut self, display_titles: Vec<String>) {
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
        self.playlist_name_input.enter_input_mode();
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
        self.list_state.select(None);
        self.album_list_state.select(None);
        self.mode = CenterPanelMode::SearchInput;
        self.search_input.enter_input_mode();
    }

    /// Open search for local filtering — keeps album songs visible while typing.
    pub fn open_search_local(&mut self) {
        self.mode = CenterPanelMode::SearchInput;
        self.search_input.enter_input_mode();
    }

    /// Filter album_songs by query and display the matches.
    pub fn filter_songs(&mut self, query: &str) {
        let query_lower = query.to_lowercase();
        self.songs = self
            .album_songs
            .iter()
            .filter(|s| s.title.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();
        self.mode = CenterPanelMode::SearchResults;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn close_search(&mut self) {
        self.search_input.exit_input_mode();
        self.restore_album_songs();
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

    pub fn is_showing_search_results(&self) -> bool {
        self.mode == CenterPanelMode::SearchResults
    }

    pub fn is_showing_album_tracks(&self) -> bool {
        self.mode == CenterPanelMode::AlbumTracks
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
            CenterPanelMode::ArtistResults => self.render_artist_results(frame, area, is_focused),
            CenterPanelMode::CreatePlaylist => self.render_create_playlist(frame, area, is_focused),
            CenterPanelMode::PlaylistPicker => self.render_playlist_picker(frame, area, is_focused),
        }
    }

    fn render_album(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let title = self
            .selected_album
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Songs");

        let list_items: Vec<ListItem> = self
            .songs
            .iter()
            .map(|s| ListItem::new(s.title.as_str()))
            .collect();

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_search_input(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let [input_area, list_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(area);

        // Search input
        let search_title = format!(" Search {} (Tab to switch) ", self.search_mode.label());
        let input_block = Block::bordered()
            .title(search_title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let input_text = Line::from(vec![
            Span::styled(
                "> ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&self.search_input.value),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]);

        let input_paragraph = Paragraph::new(input_text).block(input_block);
        frame.render_widget(input_paragraph, input_area);

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
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, list_area, &mut self.album_list_state);
    }

    fn render_search_results(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let list_items: Vec<ListItem> = self
            .songs
            .iter()
            .map(|s| ListItem::new(s.title.as_str()))
            .collect();

        let result_count = self.songs.len();
        let title = match &self.status_message {
            Some(msg) => format!("Search Results ({}) - {}", result_count, msg),
            None => format!("Search Results ({})", result_count),
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_album_results(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let list_items: Vec<ListItem> = self
            .album_display_titles
            .iter()
            .map(|t| ListItem::new(t.as_str()))
            .collect();

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
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.album_list_state);
    }

    fn render_album_tracks(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let list_items: Vec<ListItem> = self
            .songs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let num = s.track_number.unwrap_or((i + 1) as u32);
                let display = if !s.artist.is_empty() {
                    format!("{:>2}. {} - {}", num, s.artist, s.title)
                } else {
                    format!("{:>2}. {}", num, s.title)
                };
                ListItem::new(display)
            })
            .collect();

        let title = match &self.viewing_album_title {
            Some(name) => format!("{} ({} tracks)", name, self.songs.len()),
            None => format!("Tracks ({})", self.songs.len()),
        };

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_queue(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let list_items: Vec<ListItem> = self
            .queue_songs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let prefix = if i == self.queue_position { ">" } else { " " };
                let display = if !s.artist.is_empty() {
                    format!("{} {} - {}", prefix, s.artist, s.title)
                } else {
                    format!("{} {}", prefix, s.title)
                };
                let style = if i == self.queue_position {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };
                ListItem::new(display).style(style)
            })
            .collect();

        let title = format!("Queue ({} tracks)", self.queue_songs.len());

        let list = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.queue_list_state);
    }

    fn render_artist_results(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let list_items: Vec<ListItem> = self
            .artist_display_titles
            .iter()
            .map(|t| ListItem::new(t.as_str()))
            .collect();

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
            .highlight_style(Style::default().bg(Color::DarkGray));

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
            CenterPanelMode::ArtistResults => self.handle_artist_results(key),
            CenterPanelMode::CreatePlaylist => self.handle_create_playlist(key),
            CenterPanelMode::PlaylistPicker => self.handle_playlist_picker(key),
        }
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Tab => {
                self.search_mode = self.search_mode.cycle();
            }
            KeyCode::Enter => {
                let query = self.search_input.value.trim().to_string();
                if !query.is_empty() {
                    self.events
                        .push_back(CenterPanelEvent::QuerySubmitted(query));
                    self.search_input.confirm_input();
                    // Set mode based on search type so the UI shows the right view
                    self.mode = match self.search_mode {
                        SearchMode::Albums => CenterPanelMode::AlbumResults,
                        SearchMode::Artists => CenterPanelMode::ArtistResults,
                        SearchMode::Tracks => CenterPanelMode::SearchResults,
                    };
                }
            }
            KeyCode::Char(c) => self.search_input.append_char(c),
            KeyCode::Backspace => self.search_input.delete_char(),
            KeyCode::Left => self.search_input.move_cursor_left(),
            KeyCode::Right => self.search_input.move_cursor_right(),
            _ => {}
        }
    }

    fn handle_search_results(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            KeyCode::Char('/') => self.open_search(),
            KeyCode::Esc => {
                self.restore_album_songs();
                self.mode = CenterPanelMode::Album;
            }
            _ => {}
        }
    }

    fn handle_album(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            _ => {}
        }
    }

    fn handle_album_results(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_album_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_album_item(),
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
        let input_block = Block::bordered()
            .title(" New Playlist Name ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        let input_text = Line::from(vec![
            Span::styled(
                "> ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(&self.playlist_name_input.value),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ]);

        let paragraph = Paragraph::new(input_text).block(input_block);
        frame.render_widget(paragraph, area);
    }

    fn render_playlist_picker(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

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
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(list, area, &mut self.playlist_picker_state);
    }

    fn handle_create_playlist(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.playlist_name_input.exit_input_mode();
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            KeyCode::Enter => {
                let name = self.playlist_name_input.value.trim().to_string();
                if !name.is_empty() {
                    self.events
                        .push_back(CenterPanelEvent::PlaylistCreated(name));
                    self.playlist_name_input.exit_input_mode();
                    self.mode = self
                        .pre_playlist_mode
                        .take()
                        .unwrap_or(CenterPanelMode::Album);
                }
            }
            KeyCode::Char(c) => self.playlist_name_input.append_char(c),
            KeyCode::Backspace => self.playlist_name_input.delete_char(),
            KeyCode::Left => self.playlist_name_input.move_cursor_left(),
            KeyCode::Right => self.playlist_name_input.move_cursor_right(),
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
                self.mode = self
                    .pre_playlist_mode
                    .take()
                    .unwrap_or(CenterPanelMode::Album);
            }
            _ => {}
        }
    }

    fn handle_queue(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_queue_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_queue_item(),
            KeyCode::Char('d') => {
                if let Some(index) = self.queue_list_state.selected() {
                    if index != self.queue_position {
                        self.events
                            .push_back(CenterPanelEvent::QueueItemRemoved(index));
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(index) = self.queue_list_state.selected() {
                    self.events
                        .push_back(CenterPanelEvent::QueueItemJumped(index));
                }
            }
            KeyCode::Esc => {
                self.mode = self.pre_queue_mode.take().unwrap_or(CenterPanelMode::Album);
            }
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

    pub fn get_songs(&self) -> Vec<Song> {
        self.songs.clone()
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
        if self.queue_songs.is_empty() {
            return;
        }
        let i = match self.queue_list_state.selected() {
            Some(i) => {
                if i >= self.queue_songs.len() - 1 {
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
        if self.queue_songs.is_empty() {
            return;
        }
        let i = match self.queue_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.queue_songs.len() - 1
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
