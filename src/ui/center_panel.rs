use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{sources::song::Song, ui::input_line::InputLine};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CenterPanelMode {
    Album,
    SearchInput,
    SearchResults,
    AlbumResults,
    AlbumTracks,
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
    pending_query: Option<String>,
    status_message: Option<String>,
    /// Album titles for display in AlbumResults mode.
    album_display_titles: Vec<String>,
    album_list_state: ListState,
    /// The album title we're viewing tracks for (shown in AlbumTracks title bar).
    viewing_album_title: Option<String>,
    /// Signals that the user selected an album and we need to fetch its tracks.
    pending_album_selection: Option<usize>,
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
            pending_query: None,
            status_message: None,
            album_display_titles: Vec::new(),
            album_list_state: ListState::default(),
            viewing_album_title: None,
            pending_album_selection: None,
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

    /// Returns the index of a selected album in AlbumResults mode, if any.
    pub fn take_pending_album_selection(&mut self) -> Option<usize> {
        self.pending_album_selection.take()
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

    pub fn take_pending_query(&mut self) -> Option<String> {
        self.pending_query.take()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        match self.mode {
            CenterPanelMode::SearchInput => self.render_search_input(frame, area, is_focused),
            CenterPanelMode::SearchResults => self.render_search_results(frame, area, is_focused),
            CenterPanelMode::Album => self.render_album(frame, area, is_focused),
            CenterPanelMode::AlbumResults => self.render_album_results(frame, area, is_focused),
            CenterPanelMode::AlbumTracks => self.render_album_tracks(frame, area, is_focused),
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
        let input_block = Block::bordered()
            .title(" Search ")
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
            .map(|s| {
                let display = if !s.artist.is_empty() {
                    format!("{} - {}", s.artist, s.title)
                } else {
                    s.title.clone()
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

    pub fn handle_events(&mut self, key: KeyEvent) {
        match self.mode {
            CenterPanelMode::SearchInput => self.handle_search_input(key),
            CenterPanelMode::SearchResults => self.handle_search_results(key),
            CenterPanelMode::Album => self.handle_album(key),
            CenterPanelMode::AlbumResults => self.handle_album_results(key),
            CenterPanelMode::AlbumTracks => self.handle_album_tracks(key),
        }
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_search(),
            KeyCode::Enter => {
                let query = self.search_input.value.trim().to_string();
                if !query.is_empty() {
                    self.pending_query = Some(query);
                    self.search_input.confirm_input();
                    self.mode = CenterPanelMode::AlbumResults;
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
                    self.pending_album_selection = Some(index);
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
}
