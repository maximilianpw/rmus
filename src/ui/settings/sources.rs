use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};

use crate::{
    config::{Config, LocalSource, MaxStreamQuality},
    players::ShuffleMode,
    ui::{input_line::InputLine, theme, widget::selected_row_style},
};

const PAGE_STEP: usize = 10;

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
    editing_source_index: Option<usize>,
    config_dirty: bool,
    status_message: Option<String>,
}

impl SourceSettings {
    pub fn new(config: Config) -> Self {
        let sources = config.get_local_sources();
        let mut list_state = ListState::default();
        if !sources.is_empty() {
            list_state.select(Some(0));
        }

        SourceSettings {
            sources,
            config,
            list_state,
            h_scroll: 0,
            input_mode: false,
            name_input: InputLine::new(),
            path_input: InputLine::new(),
            active_field: 0,
            editing_source_index: None,
            config_dirty: false,
            status_message: None,
        }
    }

    pub fn render_sources(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let quality = self.config.audio.max_stream_quality;
        let volume = self.config.audio.default_volume.min(100);
        let shuffle = self.config.audio.default_shuffle;
        let repeat = self.config.audio.default_repeat;
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
            let input_item = ListItem::new(vec![
                Self::input_line("Name: ", &self.name_input, self.active_field == 0),
                Self::input_line("Path: ", &self.path_input, self.active_field == 1),
                Line::from(""),
            ]);
            list_items.push(input_item);

            self.list_state.select(Some(list_items.len() - 1));
        }

        let title = match &self.status_message {
            Some(msg) => format!(
                "General ({}) | Quality: {} | Volume: {}% | Shuffle: {} | Repeat: {}",
                msg,
                Self::quality_label(quality),
                volume,
                Self::shuffle_label(shuffle),
                repeat.label()
            ),
            None => format!(
                "General | Quality: {} | Volume: {}% | Shuffle: {} | Repeat: {}",
                Self::quality_label(quality),
                volume,
                Self::shuffle_label(shuffle),
                repeat.label()
            ),
        };
        let widget = List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(theme::focused_border_style(false)),
            )
            .highlight_style(selected_row_style());

        frame.render_stateful_widget(widget, area, &mut self.list_state);
    }

    fn input_line(label: &'static str, input: &InputLine, active: bool) -> Line<'static> {
        let mut spans = vec![Span::raw(label)];
        spans.extend(input.display_spans(active, theme::accent_style()));
        Line::from(spans)
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
                    self.cancel_source_input();
                }
                KeyCode::Enter => {
                    if self.save_source_form() {
                        self.name_input.confirm_input();
                        self.path_input.confirm_input();
                        self.input_mode = false;
                        self.editing_source_index = None;
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
                    self.status_message = None;
                }
                KeyCode::Backspace => {
                    active_input.delete_char();
                    self.status_message = None;
                }
                KeyCode::Delete => {
                    active_input.delete_next_char();
                    self.status_message = None;
                }
                KeyCode::Left => {
                    active_input.move_cursor_left();
                }
                KeyCode::Right => {
                    active_input.move_cursor_right();
                }
                KeyCode::Home => {
                    active_input.move_cursor_to_start();
                }
                KeyCode::End => {
                    active_input.move_cursor_to_end();
                }
                _ => {}
            }
            true
        } else {
            match key.code {
                KeyCode::Char('a') => {
                    self.begin_add_source();
                    true
                }
                KeyCode::Char('e') => {
                    self.begin_edit_source();
                    true
                }
                KeyCode::Char('q') => {
                    self.cycle_stream_quality();
                    true
                }
                KeyCode::Char('z') => {
                    self.toggle_default_shuffle();
                    true
                }
                KeyCode::Char('r') => {
                    self.cycle_default_repeat();
                    true
                }
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    self.adjust_default_volume(5);
                    true
                }
                KeyCode::Char('-') => {
                    self.adjust_default_volume(-5);
                    true
                }
                KeyCode::Char('d') => {
                    self.remove_selected_source();
                    true
                }
                KeyCode::Char('J') => {
                    self.move_selected_source_down();
                    true
                }
                KeyCode::Char('K') => {
                    self.move_selected_source_up();
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
                KeyCode::Home | KeyCode::Char('g') => {
                    self.select_first_source();
                    false
                }
                KeyCode::End | KeyCode::Char('G') => {
                    self.select_last_source();
                    false
                }
                KeyCode::PageDown => {
                    self.scroll_page_down();
                    false
                }
                KeyCode::PageUp => {
                    self.scroll_page_up();
                    false
                }
                _ => false,
            }
        }
    }

    fn move_selected_source_down(&mut self) {
        self.move_selected_source(1);
    }

    fn move_selected_source_up(&mut self) {
        self.move_selected_source(-1);
    }

    fn move_selected_source(&mut self, direction: isize) {
        if self.sources.is_empty() {
            self.status_message = Some("No source selected".to_string());
            return;
        }

        let Some(index) = self
            .list_state
            .selected()
            .filter(|index| *index < self.sources.len())
        else {
            self.status_message = Some("No source selected".to_string());
            return;
        };

        let target = if direction > 0 {
            if index + 1 >= self.sources.len() {
                self.status_message = Some("Already last source".to_string());
                return;
            }
            index + 1
        } else if index == 0 {
            self.status_message = Some("Already first source".to_string());
            return;
        } else {
            index - 1
        };

        self.sources.swap(index, target);
        self.config.local.sources = self.sources.clone();
        self.config_dirty = true;
        self.list_state.select(Some(target));
        self.h_scroll = 0;

        let name = self.sources[target].name.clone();
        let direction_label = if direction > 0 { "down" } else { "up" };
        self.status_message = match self.config.save() {
            Ok(()) => Some(format!("Moved {name} {direction_label}")),
            Err(_) => Some("Failed to save config".to_string()),
        };
    }

    fn begin_add_source(&mut self) {
        self.input_mode = true;
        self.editing_source_index = None;
        self.name_input.enter_input_mode();
        self.path_input.enter_input_mode();
        self.active_field = 0;
        self.status_message = None;
    }

    fn begin_edit_source(&mut self) {
        if self.sources.is_empty() {
            self.status_message = Some("No source selected".to_string());
            return;
        }

        let index = self
            .list_state
            .selected()
            .filter(|index| *index < self.sources.len())
            .unwrap_or(0);
        let source = self.sources[index].clone();

        self.input_mode = true;
        self.editing_source_index = Some(index);
        self.name_input.enter_input_mode();
        self.path_input.enter_input_mode();
        self.name_input.set_value(source.name);
        self.path_input
            .set_value(source.path.to_string_lossy().into_owned());
        self.active_field = 0;
        self.status_message = None;
    }

    fn cancel_source_input(&mut self) {
        let restore_index = self.editing_source_index.or_else(|| {
            if self.sources.is_empty() {
                None
            } else {
                Some(
                    self.list_state
                        .selected()
                        .unwrap_or(0)
                        .min(self.sources.len().saturating_sub(1)),
                )
            }
        });

        self.name_input.exit_input_mode();
        self.path_input.exit_input_mode();
        self.input_mode = false;
        self.editing_source_index = None;
        self.status_message = None;
        self.list_state.select(restore_index);
    }

    pub fn is_input_active(&self) -> bool {
        self.input_mode
    }

    fn scroll_down(&mut self) {
        if self.sources.is_empty() {
            self.list_state.select(None);
            self.h_scroll = 0;
            return;
        }

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
        if self.sources.is_empty() {
            self.list_state.select(None);
            self.h_scroll = 0;
            return;
        }

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

    fn select_first_source(&mut self) {
        if !self.sources.is_empty() {
            self.list_state.select(Some(0));
            self.h_scroll = 0;
        }
    }

    fn select_last_source(&mut self) {
        if !self.sources.is_empty() {
            self.list_state
                .select(Some(self.sources.len().saturating_sub(1)));
            self.h_scroll = 0;
        }
    }

    fn scroll_page_down(&mut self) {
        if !self.sources.is_empty() {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state.select(Some(
                current
                    .saturating_add(PAGE_STEP)
                    .min(self.sources.len() - 1),
            ));
            self.h_scroll = 0;
        }
    }

    fn scroll_page_up(&mut self) {
        if !self.sources.is_empty() {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state
                .select(Some(current.saturating_sub(PAGE_STEP)));
            self.h_scroll = 0;
        }
    }

    fn remove_selected_source(&mut self) {
        if self.sources.is_empty() {
            self.status_message = Some("No source selected".to_string());
            return;
        }

        let index = self
            .list_state
            .selected()
            .filter(|index| *index < self.sources.len())
            .unwrap_or(0);
        let removed = self.sources.remove(index);
        self.config.local.sources.remove(index);
        self.config_dirty = true;

        if self.sources.is_empty() {
            self.list_state.select(None);
        } else {
            self.list_state
                .select(Some(index.min(self.sources.len().saturating_sub(1))));
        }

        self.status_message = match self.config.save() {
            Ok(()) => Some(format!("Removed {}", removed.name)),
            Err(_) => Some("Failed to save config".to_string()),
        };
    }

    fn save_source_form(&mut self) -> bool {
        let editing_index = match self.editing_source_index {
            Some(index) if index < self.sources.len() => Some(index),
            Some(_) => {
                self.status_message = Some("No source selected".to_string());
                return false;
            }
            None => None,
        };
        let name = self.name_input.value.trim().to_string();
        let raw_path = self.path_input.value.trim();
        if name.is_empty() || raw_path.is_empty() {
            self.status_message = Some("Name/path required".to_string());
            return false;
        }

        if self
            .sources
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != editing_index)
            .any(|(_, source)| source.name.eq_ignore_ascii_case(&name))
        {
            self.status_message = Some("Source name already exists".to_string());
            return false;
        }

        let raw_path = crate::utils::expand_home_path(std::path::Path::new(raw_path));
        if !raw_path.is_dir() {
            self.status_message = Some("Path must be an existing directory".to_string());
            return false;
        }
        let path = raw_path.canonicalize().unwrap_or(raw_path);
        if self
            .sources
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != editing_index)
            .any(|(_, source)| {
                source
                    .path
                    .canonicalize()
                    .map_or(source.path == path, |existing| existing == path)
            })
        {
            self.status_message = Some("Source already exists".to_string());
            return false;
        }

        let source = LocalSource {
            name: name.clone(),
            path: path.clone(),
        };
        if let Some(index) = editing_index {
            self.sources[index] = source;
            self.config.local.sources = self.sources.clone();
        } else {
            self.sources.push(source);
            self.config.add_local_source(name, path);
        }
        self.config_dirty = true;

        if self.config.save().is_ok() {
            self.name_input.value.clear();
            self.path_input.value.clear();
            self.list_state.select(Some(
                editing_index.unwrap_or_else(|| self.sources.len().saturating_sub(1)),
            ));
            self.status_message = Some(if editing_index.is_some() {
                "Updated".to_string()
            } else {
                "Saved".to_string()
            });
            true
        } else {
            self.status_message = Some("Failed to save config".to_string());
            false
        }
    }

    fn quality_label(quality: MaxStreamQuality) -> &'static str {
        match quality {
            MaxStreamQuality::Mp3 => "MP3",
            MaxStreamQuality::Cd => "CD",
            MaxStreamQuality::HiRes => "Hi-Res",
        }
    }

    fn shuffle_label(shuffle: ShuffleMode) -> &'static str {
        match shuffle {
            ShuffleMode::Off => "Off",
            ShuffleMode::On => "On",
        }
    }

    fn cycle_stream_quality(&mut self) {
        let current = self.config.audio.max_stream_quality;
        self.config.audio.max_stream_quality = match current {
            MaxStreamQuality::Mp3 => MaxStreamQuality::Cd,
            MaxStreamQuality::Cd => MaxStreamQuality::HiRes,
            MaxStreamQuality::HiRes => MaxStreamQuality::Mp3,
        };
        self.config_dirty = true;
        self.status_message = match self.config.save() {
            Ok(()) => Some(format!(
                "Quality set to {}",
                Self::quality_label(self.config.audio.max_stream_quality)
            )),
            Err(_) => Some("Failed to save config".to_string()),
        };
    }

    fn adjust_default_volume(&mut self, amount: i16) {
        let current = self.config.audio.default_volume.min(100) as i16;
        self.config.audio.default_volume = (current + amount).clamp(0, 100) as u16;
        self.config_dirty = true;
        self.status_message = match self.config.save() {
            Ok(()) => Some(format!(
                "Startup volume set to {}%",
                self.config.audio.default_volume
            )),
            Err(_) => Some("Failed to save config".to_string()),
        };
    }

    fn toggle_default_shuffle(&mut self) {
        self.config.audio.default_shuffle = match self.config.audio.default_shuffle {
            ShuffleMode::Off => ShuffleMode::On,
            ShuffleMode::On => ShuffleMode::Off,
        };
        self.config_dirty = true;
        self.status_message = match self.config.save() {
            Ok(()) => Some(format!(
                "Startup shuffle {}",
                Self::shuffle_label(self.config.audio.default_shuffle)
            )),
            Err(_) => Some("Failed to save config".to_string()),
        };
    }

    fn cycle_default_repeat(&mut self) {
        self.config.audio.default_repeat = self.config.audio.default_repeat.cycle();
        self.config_dirty = true;
        self.status_message = match self.config.save() {
            Ok(()) => Some(format!(
                "Startup repeat {}",
                self.config.audio.default_repeat.label()
            )),
            Err(_) => Some("Failed to save config".to_string()),
        };
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
        if self.sources.is_empty() {
            self.list_state.select(None);
        } else if self
            .list_state
            .selected()
            .is_none_or(|index| index >= self.sources.len())
        {
            self.list_state.select(Some(0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AudioConfig, LocalConfig, MaxStreamQuality};
    use crate::players::RepeatMode;
    use crossterm::event::{KeyEvent, KeyModifiers};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

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
                default_shuffle: ShuffleMode::Off,
                default_repeat: RepeatMode::Off,
            },
        }
    }

    fn unique_temp_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rmus-source-settings-{}-{}-{}",
            std::process::id(),
            nonce,
            counter
        ));
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
        assert_eq!(updated.local.sources[0].path, dir.canonicalize().unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn source_text_inputs_support_home_end_and_delete() {
        let mut settings = SourceSettings::new(default_config());
        let dir = unique_temp_dir();
        let dir_str = format!("X{}", dir.to_string_lossy());

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "XLibrary".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Home));
        settings.handle_events(key(KeyCode::Delete));
        settings.handle_events(key(KeyCode::End));
        for c in " Source".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }

        settings.handle_events(key(KeyCode::Tab));
        for c in dir_str.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Home));
        settings.handle_events(key(KeyCode::Delete));
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("source should produce config update");
        assert_eq!(updated.local.sources.len(), 1);
        assert_eq!(updated.local.sources[0].name, "Library Source");
        assert_eq!(updated.local.sources[0].path, dir.canonicalize().unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn persists_canonical_source_path() {
        let mut settings = SourceSettings::new(default_config());
        let dir = unique_temp_dir();
        let entered = dir.join(".").to_string_lossy().to_string();

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "Canonical".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in entered.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("source should produce config update");
        assert_eq!(updated.local.sources[0].path, dir.canonicalize().unwrap());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_tilde_source_path() {
        let mut settings = SourceSettings::new(default_config());
        let home = directories::BaseDirs::new()
            .expect("home directory should be available for tests")
            .home_dir()
            .to_path_buf();
        let dir_name = format!(
            ".rmus-source-settings-home-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let dir = home.join(&dir_name);
        fs::create_dir_all(&dir).unwrap();
        let entered = format!("~/{}", dir_name);

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "Home Music".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in entered.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("source should produce config update");
        assert_eq!(updated.local.sources[0].path, dir.canonicalize().unwrap());

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
        assert!(
            settings.input_mode,
            "should stay in input mode on invalid path"
        );
    }

    #[test]
    fn editing_source_input_clears_validation_message() {
        let mut settings = SourceSettings::new(default_config());

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        settings.handle_events(key(KeyCode::Enter));
        assert_eq!(
            settings.status_message.as_deref(),
            Some("Name/path required")
        );

        settings.handle_events(key(KeyCode::Char('N')));
        assert_eq!(settings.status_message, None);

        settings.handle_events(key(KeyCode::Enter));
        assert_eq!(
            settings.status_message.as_deref(),
            Some("Name/path required")
        );

        settings.handle_events(key(KeyCode::Backspace));
        assert_eq!(settings.status_message, None);
    }

    #[test]
    fn scrolling_empty_source_list_keeps_selection_empty() {
        let mut settings = SourceSettings::new(default_config());

        settings.handle_events(key(KeyCode::Down));
        assert_eq!(settings.list_state.selected(), None);

        settings.handle_events(key(KeyCode::Up));
        assert_eq!(settings.list_state.selected(), None);
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_source_rows() {
        let mut config = default_config();
        for n in 1..=3 {
            config.add_local_source(format!("Source {n}"), PathBuf::from(format!("/music/{n}")));
        }
        let mut settings = SourceSettings::new(config);

        settings.handle_events(key(KeyCode::End));
        assert_eq!(settings.list_state.selected(), Some(2));

        settings.handle_events(key(KeyCode::Home));
        assert_eq!(settings.list_state.selected(), Some(0));

        settings.handle_events(key(KeyCode::Char('G')));
        assert_eq!(settings.list_state.selected(), Some(2));

        settings.handle_events(key(KeyCode::Char('g')));
        assert_eq!(settings.list_state.selected(), Some(0));
    }

    #[test]
    fn page_up_and_page_down_move_source_selection_by_page() {
        let mut config = default_config();
        for n in 1..=12 {
            config.add_local_source(format!("Source {n}"), PathBuf::from(format!("/music/{n}")));
        }
        let mut settings = SourceSettings::new(config);

        settings.handle_events(key(KeyCode::PageDown));
        assert_eq!(settings.list_state.selected(), Some(10));

        settings.handle_events(key(KeyCode::PageDown));
        assert_eq!(settings.list_state.selected(), Some(11));

        settings.handle_events(key(KeyCode::PageUp));
        assert_eq!(settings.list_state.selected(), Some(1));

        settings.handle_events(key(KeyCode::PageUp));
        assert_eq!(settings.list_state.selected(), Some(0));
    }

    #[test]
    fn rejects_duplicate_source_path() {
        let dir = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("Existing".to_string(), dir.clone());
        let mut settings = SourceSettings::new(config);
        let dir_str = dir.to_string_lossy().to_string();

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "Duplicate".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in dir_str.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        assert!(
            settings.take_config_update().is_none(),
            "duplicate source should not be persisted"
        );
        assert_eq!(settings.sources.len(), 1);
        assert!(
            settings.input_mode,
            "should stay in input mode when the source already exists"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_duplicate_source_path_after_canonicalization() {
        let dir = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("Existing".to_string(), dir.canonicalize().unwrap());
        let mut settings = SourceSettings::new(config);
        let entered = dir.join(".").to_string_lossy().to_string();

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "Duplicate".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in entered.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        assert!(
            settings.take_config_update().is_none(),
            "canonical duplicate source should not be persisted"
        );
        assert_eq!(settings.sources.len(), 1);
        assert!(
            settings.input_mode,
            "should stay in input mode when the canonical source already exists"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_duplicate_source_name_case_insensitively() {
        let first = unique_temp_dir();
        let second = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("Library".to_string(), first.clone());
        let mut settings = SourceSettings::new(config);
        let second_str = second.to_string_lossy().to_string();

        assert!(settings.handle_events(key(KeyCode::Char('a'))));
        for c in "library".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in second_str.chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        assert!(
            settings.take_config_update().is_none(),
            "duplicate source name should not be persisted"
        );
        assert_eq!(settings.sources.len(), 1);
        assert_eq!(
            settings.status_message.as_deref(),
            Some("Source name already exists")
        );
        assert!(
            settings.input_mode,
            "should stay in input mode when the source name already exists"
        );

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn edits_selected_source_and_persists_config() {
        let first = unique_temp_dir();
        let second = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("Old Library".to_string(), first.clone());
        let mut settings = SourceSettings::new(config);

        assert!(settings.handle_events(key(KeyCode::Char('e'))));
        assert!(settings.is_input_active());
        assert_eq!(settings.name_input.value, "Old Library");
        assert_eq!(settings.path_input.value, first.to_string_lossy());

        settings.name_input.set_value("New Library".to_string());
        settings
            .path_input
            .set_value(second.to_string_lossy().to_string());
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("editing a source should produce config update");
        assert_eq!(updated.local.sources.len(), 1);
        assert_eq!(updated.local.sources[0].name, "New Library");
        assert_eq!(
            updated.local.sources[0].path,
            second.canonicalize().unwrap()
        );
        assert_eq!(settings.sources[0].name, "New Library");
        assert_eq!(settings.sources[0].path, second.canonicalize().unwrap());
        assert_eq!(settings.list_state.selected(), Some(0));
        assert!(!settings.is_input_active());
        assert_eq!(settings.status_message.as_deref(), Some("Updated"));

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn editing_source_allows_unchanged_path() {
        let dir = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("Library".to_string(), dir.clone());
        let mut settings = SourceSettings::new(config);

        assert!(settings.handle_events(key(KeyCode::Char('e'))));
        settings.name_input.set_value("Renamed Library".to_string());
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("renaming a source should produce config update");
        assert_eq!(updated.local.sources[0].name, "Renamed Library");
        assert_eq!(updated.local.sources[0].path, dir.canonicalize().unwrap());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn editing_source_rejects_duplicate_source_name() {
        let first = unique_temp_dir();
        let second = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("First".to_string(), first.clone());
        config.add_local_source("Second".to_string(), second.clone());
        let mut settings = SourceSettings::new(config);

        settings.handle_events(key(KeyCode::Down));
        assert_eq!(settings.list_state.selected(), Some(1));

        assert!(settings.handle_events(key(KeyCode::Char('e'))));
        settings.name_input.set_value("first".to_string());
        settings.handle_events(key(KeyCode::Enter));

        assert!(
            settings.take_config_update().is_none(),
            "duplicate edit should not be persisted"
        );
        assert_eq!(settings.sources[1].name, "Second");
        assert_eq!(
            settings.status_message.as_deref(),
            Some("Source name already exists")
        );
        assert!(
            settings.input_mode,
            "should stay in input mode when the source name already exists"
        );

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn cycles_stream_quality_and_marks_config_dirty() {
        let mut settings = SourceSettings::new(default_config());
        assert_eq!(
            settings.config.audio.max_stream_quality,
            MaxStreamQuality::HiRes
        );

        assert!(settings.handle_events(key(KeyCode::Char('q'))));
        assert_eq!(
            settings.config.audio.max_stream_quality,
            MaxStreamQuality::Mp3
        );

        let updated = settings
            .take_config_update()
            .expect("config update expected");
        assert_eq!(updated.audio.max_stream_quality, MaxStreamQuality::Mp3);
    }

    #[test]
    fn removes_selected_source_and_marks_config_dirty() {
        let first = unique_temp_dir();
        let second = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("First".to_string(), first.clone());
        config.add_local_source("Second".to_string(), second.clone());
        let mut settings = SourceSettings::new(config);

        assert!(settings.handle_events(key(KeyCode::Char('d'))));

        let updated = settings
            .take_config_update()
            .expect("removing a source should produce config update");
        assert_eq!(updated.local.sources.len(), 1);
        assert_eq!(updated.local.sources[0].name, "Second");
        assert_eq!(settings.sources.len(), 1);
        assert_eq!(settings.sources[0].path, second);

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn moves_selected_source_down_and_up_and_persists_order() {
        let first = unique_temp_dir();
        let second = unique_temp_dir();
        let third = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("First".to_string(), first.clone());
        config.add_local_source("Second".to_string(), second.clone());
        config.add_local_source("Third".to_string(), third.clone());
        let mut settings = SourceSettings::new(config);

        assert!(settings.handle_events(key(KeyCode::Char('J'))));
        let updated = settings
            .take_config_update()
            .expect("moving a source should produce config update");
        let names: Vec<_> = updated
            .local
            .sources
            .iter()
            .map(|source| source.name.as_str())
            .collect();
        assert_eq!(names, vec!["Second", "First", "Third"]);
        assert_eq!(settings.list_state.selected(), Some(1));
        assert_eq!(settings.status_message.as_deref(), Some("Moved First down"));

        assert!(settings.handle_events(key(KeyCode::Char('K'))));
        let updated = settings
            .take_config_update()
            .expect("moving a source should produce config update");
        let names: Vec<_> = updated
            .local
            .sources
            .iter()
            .map(|source| source.name.as_str())
            .collect();
        assert_eq!(names, vec!["First", "Second", "Third"]);
        assert_eq!(settings.list_state.selected(), Some(0));
        assert_eq!(settings.status_message.as_deref(), Some("Moved First up"));

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
        let _ = fs::remove_dir_all(third);
    }

    #[test]
    fn source_move_boundaries_show_feedback_without_dirtying_config() {
        let first = unique_temp_dir();
        let second = unique_temp_dir();
        let mut config = default_config();
        config.add_local_source("First".to_string(), first.clone());
        config.add_local_source("Second".to_string(), second.clone());
        let mut settings = SourceSettings::new(config);

        assert!(settings.handle_events(key(KeyCode::Char('K'))));
        assert!(settings.take_config_update().is_none());
        assert_eq!(settings.list_state.selected(), Some(0));
        assert_eq!(
            settings.status_message.as_deref(),
            Some("Already first source")
        );

        settings.handle_events(key(KeyCode::End));
        assert!(settings.handle_events(key(KeyCode::Char('J'))));
        assert!(settings.take_config_update().is_none());
        assert_eq!(settings.list_state.selected(), Some(1));
        assert_eq!(
            settings.status_message.as_deref(),
            Some("Already last source")
        );

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn adjusts_default_volume_and_marks_config_dirty() {
        let mut settings = SourceSettings::new(default_config());

        assert!(settings.handle_events(key(KeyCode::Char('+'))));
        let updated = settings
            .take_config_update()
            .expect("volume increase should produce config update");
        assert_eq!(updated.audio.default_volume, 55);

        assert!(settings.handle_events(key(KeyCode::Char('-'))));
        let updated = settings
            .take_config_update()
            .expect("volume decrease should produce config update");
        assert_eq!(updated.audio.default_volume, 50);
    }

    #[test]
    fn toggles_startup_shuffle_and_cycles_startup_repeat() {
        let mut settings = SourceSettings::new(default_config());

        assert!(settings.handle_events(key(KeyCode::Char('z'))));
        let updated = settings
            .take_config_update()
            .expect("shuffle toggle should produce config update");
        assert_eq!(updated.audio.default_shuffle, ShuffleMode::On);

        assert!(settings.handle_events(key(KeyCode::Char('r'))));
        let updated = settings
            .take_config_update()
            .expect("repeat cycle should produce config update");
        assert_eq!(updated.audio.default_repeat, RepeatMode::All);

        assert!(settings.handle_events(key(KeyCode::Char('r'))));
        let updated = settings
            .take_config_update()
            .expect("repeat cycle should produce config update");
        assert_eq!(updated.audio.default_repeat, RepeatMode::One);
    }
}
