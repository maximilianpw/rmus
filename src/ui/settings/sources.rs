use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    config::{Config, LocalSource},
    ui::input_line::InputLine,
};

#[derive(Debug, Default)]
pub struct SourceSettings {
    config: Config,
    sources: Vec<LocalSource>,
    list_state: ListState,
    h_scroll: usize,
    input_mode: bool,
    name_input: InputLine,
    path_input: InputLine,
    active_field: usize, // 0 = name, 1 = path
    config_dirty: bool,
}

impl SourceSettings {
    pub fn new(config: Config) -> Self {
        SourceSettings {
            sources: config.get_local_sources(),
            config,
            list_state: ListState::default(),
            h_scroll: 0,
            input_mode: false,
            name_input: InputLine::new(),
            path_input: InputLine::new(),
            active_field: 0,
            config_dirty: false,
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
                Line::from(format!("Name: {}{}", self.name_input.value, name_cursor)),
                Line::from(format!("Path: {}{}", self.path_input.value, path_cursor)),
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
            let active_input = if self.active_field == 0 {
                &mut self.name_input
            } else {
                &mut self.path_input
            };

            match key.code {
                KeyCode::Esc => {
                    self.name_input.exit_input_mode();
                    self.path_input.exit_input_mode();
                    self.input_mode = false;
                }
                KeyCode::Enter => {
                    if self.save_new_source() {
                        self.name_input.confirm_input();
                        self.path_input.confirm_input();
                        self.input_mode = false;
                    }
                }
                KeyCode::Tab | KeyCode::Down => {
                    self.active_field = (self.active_field + 1) % 2;
                }
                KeyCode::BackTab | KeyCode::Up => {
                    self.active_field = if self.active_field == 0 { 1 } else { 0 };
                }
                KeyCode::Char(c) => {
                    active_input.append_char(c);
                }
                KeyCode::Backspace => {
                    active_input.delete_char();
                }
                KeyCode::Left => {
                    active_input.move_cursor_left();
                }
                KeyCode::Right => {
                    active_input.move_cursor_right();
                }
                _ => {}
            }
            true
        } else {
            match key.code {
                KeyCode::Char('a') => {
                    self.input_mode = true;
                    self.name_input.enter_input_mode();
                    self.path_input.enter_input_mode();
                    self.active_field = 0;
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

    fn save_new_source(&mut self) -> bool {
        let name = self.name_input.value.trim().to_string();
        let raw_path = self.path_input.value.trim();
        if name.is_empty() || raw_path.is_empty() {
            return false;
        }

        let path = PathBuf::from(raw_path);
        if !path.is_dir() {
            return false;
        }

        let source = LocalSource {
            name: name.clone(),
            path: path.clone(),
        };
        self.sources.push(source.clone());
        self.config.add_local_source(name, path);
        self.config_dirty = true;

        if self.config.save().is_ok() {
            self.name_input.value.clear();
            self.path_input.value.clear();
            self.list_state
                .select(Some(self.sources.len().saturating_sub(1)));
            true
        } else {
            false
        }
    }

    pub fn take_config_update(&mut self) -> Option<Config> {
        if self.config_dirty {
            self.config_dirty = false;
            Some(self.config.clone())
        } else {
            None
        }
    }

    pub fn update_config(&mut self, config: &Config) {
        self.config = config.clone();
        self.sources = self.config.get_local_sources();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AudioConfig, LocalConfig};
    use crossterm::event::{KeyEvent, KeyModifiers};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            audio: AudioConfig { default_volume: 50 },
        }
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rmus-source-settings-{}", nonce));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn adds_and_persists_source_on_enter() {
        let mut settings = SourceSettings::new(default_config());
        let dir = unique_temp_dir();
        let dir_str = dir.to_string_lossy().to_string();

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "Test Album".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in dir_str.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("source should produce config update");
        assert_eq!(updated.local.sources.len(), 1);
        assert_eq!(updated.local.sources[0].name, "Test Album");
        assert_eq!(updated.local.sources[0].path, dir);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_source_path() {
        let mut settings = SourceSettings::new(default_config());
        let invalid = std::env::temp_dir()
            .join("rmus-source-settings-does-not-exist")
            .to_string_lossy()
            .to_string();

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "Broken".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in invalid.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        assert!(
            settings.take_config_update().is_none(),
            "invalid source should not be persisted"
        );
        assert_eq!(settings.sources.len(), 0);
        assert!(settings.input_mode, "should stay in input mode on invalid path");
    }
}
