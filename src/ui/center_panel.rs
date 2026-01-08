use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{sources::song::Song, ui::log_panel::Logger};

#[derive(Debug)]
pub struct CenterPanel {
    selected_album: Option<PathBuf>,
    songs: Vec<Song>,
    list_state: ListState,
    logger: Logger,
}

impl CenterPanel {
    pub fn new(logger: Logger) -> Self {
        Self {
            selected_album: None,
            songs: Vec::new(),
            list_state: ListState::default(),
            logger,
        }
    }

    pub fn set_album(&mut self, path: PathBuf, songs: Vec<Song>) {
        self.selected_album = Some(path);
        self.songs = songs;
        if !self.songs.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
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

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
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
}
