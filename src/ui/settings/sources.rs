use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::config::{Config, LocalSource};

#[derive(Debug, Default)]
pub struct SourceSettings {
    config: Config,
    sources: Vec<LocalSource>,
    list_state: ListState,
    h_scroll: usize,
    input_mode: bool,
    name_input: String,
    path_input: String,
    active_field: usize, // 0 = name, 1 = path
}

impl SourceSettings {
    pub fn new(config: Config) -> Self {
        SourceSettings {
            sources: config.get_local_sources(),
            config,
            list_state: ListState::default(),
            h_scroll: 0,
            input_mode: false,
            name_input: String::new(),
            path_input: String::new(),
            active_field: 0,
        }
    }

    pub fn render_sources(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let mut list_items: Vec<ListItem> = self
            .sources
            .iter()
            .map(|s| {
                ListItem::new(vec![
                    Line::from(s.name.clone()),
                    Line::from(s.path.to_string_lossy().into_owned()),
                    Line::from(""),
                ])
            })
            .collect();

        if self.input_mode {
            let name_cursor = if self.active_field == 0 { "_" } else { "" };
            let path_cursor = if self.active_field == 1 { "_" } else { "" };

            let input_item = ListItem::new(vec![
                Line::from(format!("Name: {}{}", self.name_input, name_cursor)),
                Line::from(format!("Path: {}{}", self.path_input, path_cursor)),
                Line::from(""),
            ]);
            list_items.push(input_item);

            self.list_state.select(Some(list_items.len() - 1));
        }

        let widget = List::new(list_items)
            .block(Block::bordered().title("Library").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray));

        frame.render_stateful_widget(widget, area, &mut self.list_state);
    }

    pub fn handle_events(&mut self, key: KeyEvent) -> bool {
        if self.input_mode {
            match key.code {
                KeyCode::Esc => {
                    self.exit_input_mode();
                }
                KeyCode::Enter => {
                    self.confirm_input();
                }
                KeyCode::Tab | KeyCode::Down => {
                    self.active_field = (self.active_field + 1) % 2;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    self.active_field = if self.active_field == 0 { 1 } else { 0 };
                }
                KeyCode::Char(c) => {
                    self.append_char(c);
                }
                KeyCode::Backspace => {
                    self.delete_char();
                }
                _ => {}
            }
            true
        } else {
            match key.code {
                KeyCode::Char('a') => {
                    self.enter_input_mode();
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.scroll_down();
                    false
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.scroll_up();
                    false
                }
                _ => false,
            }
        }
    }

    fn enter_input_mode(&mut self) {
        self.input_mode = true;
        self.name_input.clear();
        self.path_input.clear();
        self.active_field = 0;
    }

    fn exit_input_mode(&mut self) {
        self.input_mode = false;
        self.name_input.clear();
        self.path_input.clear();
        self.active_field = 0;
    }

    fn confirm_input(&mut self) {
        if !self.name_input.is_empty() && !self.path_input.is_empty() {
            let name = self.name_input.clone();
            let path = PathBuf::from(&self.path_input);

            self.config.add_local_source(name.clone(), path.clone());
            let _ = self.config.save();

            self.sources.push(LocalSource { name, path });
        }
        self.exit_input_mode();
    }

    fn append_char(&mut self, c: char) {
        if self.active_field == 0 {
            self.name_input.push(c);
        } else {
            self.path_input.push(c);
        }
    }

    fn delete_char(&mut self) {
        if self.active_field == 0 {
            self.name_input.pop();
        } else {
            self.path_input.pop();
        }
    }

    fn scroll_down(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.sources.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.h_scroll = 0;
    }

    fn scroll_up(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.sources.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.h_scroll = 0;
    }
}
