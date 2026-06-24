use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{
    sources::{song::Song, MusicSource},
    ui::{
        input_line::InputLine,
        log_panel::Logger,
        theme,
        widget::{handle_focused_border_style, selected_row_style, tabs_from_strings},
    },
};

pub const TABS_HEIGHT: u16 = 3;
const PAGE_STEP: usize = 10;

#[derive(Debug)]
pub struct LeftPanel {
    selected_tab_index: usize,
    items: Vec<Box<dyn MusicSource>>,
    list_state: ListState,
    all_items: Vec<String>,
    cached_items: Vec<String>,
    visible_indices: Vec<usize>,
    filter_input: InputLine,
    status_line: Option<String>,
    logger: Logger,
}

impl LeftPanel {
    pub fn new(sources: Vec<Box<dyn MusicSource>>, logger: Logger) -> Self {
        let mut panel = Self {
            selected_tab_index: 0,
            items: sources,
            list_state: ListState::default(),
            all_items: Vec::new(),
            cached_items: Vec::new(),
            visible_indices: Vec::new(),
            filter_input: InputLine::new(),
            status_line: None,
            logger,
        };
        panel.update_cache();
        panel
    }

    pub fn update_cache(&mut self) {
        if let Some(sources) = self.items.get(self.selected_tab_index) {
            self.all_items = sources.get_albums();
        } else {
            self.all_items.clear();
        }

        self.apply_filter_to_cache();
    }

    fn apply_filter_to_cache(&mut self) {
        let query = self.filter_input.value.trim().to_lowercase();
        self.cached_items.clear();
        self.visible_indices.clear();

        for (index, item) in self.all_items.iter().enumerate() {
            if query.is_empty() || item.to_lowercase().contains(&query) {
                self.cached_items.push(item.clone());
                self.visible_indices.push(index);
            }
        }

        match self.list_state.selected() {
            Some(index) if index < self.cached_items.len() => {}
            _ if !self.cached_items.is_empty() => self.list_state.select(Some(0)),
            _ => self.list_state.select(None),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let show_filter = self.filter_input.is_input_mode() || self.has_filter_query();
        let show_status = self
            .status_line
            .as_deref()
            .is_some_and(|line| !line.trim().is_empty());
        let (tabs_area, filter_area, status_area, list_area) = match (show_filter, show_status) {
            (true, true) => {
                let layout = Layout::vertical([
                    Constraint::Length(TABS_HEIGHT),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                ]);
                let [tabs_area, filter_area, status_area, list_area] = layout.areas(area);
                (tabs_area, Some(filter_area), Some(status_area), list_area)
            }
            (true, false) => {
                let layout = Layout::vertical([
                    Constraint::Length(TABS_HEIGHT),
                    Constraint::Length(3),
                    Constraint::Fill(1),
                ]);
                let [tabs_area, filter_area, list_area] = layout.areas(area);
                (tabs_area, Some(filter_area), None, list_area)
            }
            (false, true) => {
                let layout = Layout::vertical([
                    Constraint::Length(TABS_HEIGHT),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                ]);
                let [tabs_area, status_area, list_area] = layout.areas(area);
                (tabs_area, None, Some(status_area), list_area)
            }
            (false, false) => {
                let layout =
                    Layout::vertical([Constraint::Length(TABS_HEIGHT), Constraint::Fill(1)]);
                let [tabs_area, list_area] = layout.areas(area);
                (tabs_area, None, None, list_area)
            }
        };
        let tab_names: Vec<String> = self.items.iter().map(|s| s.name()).collect();

        let tabs = tabs_from_strings(&tab_names, self.selected_tab_index, is_focused);
        frame.render_widget(tabs, tabs_area);

        if let Some(filter_area) = filter_area {
            self.render_filter(frame, filter_area, is_focused);
        }
        if let Some(status_area) = status_area {
            self.render_status(frame, status_area);
        }

        let list_items = if self.cached_items.is_empty() {
            self.empty_tab_items()
        } else {
            self.cached_items
                .iter()
                .map(|item| ListItem::new(Line::from(item.as_str())))
                .collect()
        };
        let list = self.list_from_items(list_items, is_focused);
        frame.render_stateful_widget(list, list_area, &mut self.list_state);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        if self.filter_input.is_input_mode() {
            self.handle_filter_input(key);
            return;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.next_item(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_item(),
            KeyCode::Home | KeyCode::Char('g') => self.first_item(),
            KeyCode::End | KeyCode::Char('G') => self.last_item(),
            KeyCode::PageDown => self.next_page(),
            KeyCode::PageUp => self.previous_page(),
            KeyCode::Char('l') | KeyCode::Right => self.next_tab(),
            KeyCode::Char('h') | KeyCode::Left => self.previous_tab(),
            KeyCode::Esc if self.handles_escape() => self.clear_filter(),
            _ => {}
        }
    }

    fn handle_filter_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.clear_filter(),
            KeyCode::Enter => self.filter_input.confirm_input(),
            KeyCode::Char(c) => {
                self.filter_input.append_char(c);
                self.apply_filter_to_cache();
            }
            KeyCode::Backspace => {
                self.filter_input.delete_char();
                self.apply_filter_to_cache();
            }
            KeyCode::Delete => {
                self.filter_input.delete_next_char();
                self.apply_filter_to_cache();
            }
            KeyCode::Left => self.filter_input.move_cursor_left(),
            KeyCode::Right => self.filter_input.move_cursor_right(),
            KeyCode::Home => self.filter_input.move_cursor_to_start(),
            KeyCode::End => self.filter_input.move_cursor_to_end(),
            KeyCode::Down => self.next_item(),
            KeyCode::Up => self.previous_item(),
            KeyCode::PageDown => self.next_page(),
            KeyCode::PageUp => self.previous_page(),
            _ => {}
        }
    }

    fn next_item(&mut self) {
        if self.cached_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.cached_items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn first_item(&mut self) {
        if !self.cached_items.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn last_item(&mut self) {
        if !self.cached_items.is_empty() {
            self.list_state
                .select(Some(self.cached_items.len().saturating_sub(1)));
        }
    }

    fn previous_item(&mut self) {
        if self.cached_items.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.cached_items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn next_page(&mut self) {
        if !self.cached_items.is_empty() {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state.select(Some(
                current
                    .saturating_add(PAGE_STEP)
                    .min(self.cached_items.len() - 1),
            ));
        }
    }

    fn previous_page(&mut self) {
        if !self.cached_items.is_empty() {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state
                .select(Some(current.saturating_sub(PAGE_STEP)));
        }
    }

    pub fn get_selected_album(&self) -> Option<(PathBuf, Vec<Song>)> {
        let visible_idx = self.list_state.selected()?;
        let idx = *self.visible_indices.get(visible_idx)?;
        let source = self.items.get(self.selected_tab_index)?;
        let path = source.get_album_path(idx)?;
        let songs = source.get_songs_from_album(path.clone());
        Some((path, songs))
    }

    pub fn active_tab_name(&self) -> String {
        self.items
            .get(self.selected_tab_index)
            .map(|s| s.name())
            .unwrap_or_default()
    }

    pub fn select_tab_by_name(&mut self, tab_name: &str) {
        let Some(index) = self
            .items
            .iter()
            .position(|source| source.name() == tab_name)
        else {
            return;
        };

        self.filter_input.exit_input_mode();
        self.selected_tab_index = index;
        self.list_state.select(Some(0));
        self.update_cache();
    }

    pub fn selected_item_index(&self) -> Option<usize> {
        self.list_state
            .selected()
            .and_then(|index| self.visible_indices.get(index))
            .copied()
    }

    pub fn selected_item_label(&self) -> Option<String> {
        self.list_state
            .selected()
            .and_then(|index| self.cached_items.get(index))
            .cloned()
    }

    pub fn can_filter_active_tab(&self) -> bool {
        matches!(self.active_tab_name().as_str(), "Local" | "Playlists")
            && !self.all_items.is_empty()
    }

    pub fn open_filter(&mut self) {
        self.filter_input.enter_input_mode();
        self.apply_filter_to_cache();
    }

    pub fn is_filter_input_active(&self) -> bool {
        self.filter_input.is_input_mode()
    }

    pub fn set_status_line(&mut self, status_line: Option<String>) {
        self.status_line = status_line;
    }

    pub fn handles_escape(&self) -> bool {
        self.filter_input.is_input_mode() || self.has_filter_query()
    }

    fn clear_filter(&mut self) {
        self.filter_input.exit_input_mode();
        self.apply_filter_to_cache();
    }

    fn has_filter_query(&self) -> bool {
        !self.filter_input.value.trim().is_empty()
    }

    fn render_filter(&self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = theme::focused_border_style(is_focused);
        let mut input_spans = vec![Span::styled("/ ", theme::accent_bold_style())];
        input_spans.extend(
            self.filter_input
                .display_spans(self.filter_input.is_input_mode(), theme::accent_style()),
        );
        let paragraph = Paragraph::new(Line::from(input_spans)).block(
            Block::bordered()
                .title(" Filter List ")
                .borders(Borders::ALL)
                .border_style(border_style),
        );
        frame.render_widget(paragraph, area);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let Some(status) = self.status_line.as_deref() else {
            return;
        };
        let line = Line::from(vec![
            Span::styled("Scan: ", theme::section_style()),
            Span::styled(status.to_string(), theme::info_style()),
        ]);
        frame.render_widget(Paragraph::new(line), area);
    }

    fn list_from_items<'a>(&self, list_items: Vec<ListItem<'a>>, is_focused: bool) -> List<'a> {
        let title = if self.has_filter_query() {
            format!("Library ({} matches)", self.cached_items.len())
        } else {
            "Library".to_string()
        };
        List::new(list_items)
            .block(
                Block::bordered()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(handle_focused_border_style(is_focused)),
            )
            .highlight_style(selected_row_style())
    }

    fn empty_tab_items(&self) -> Vec<ListItem<'static>> {
        let lines: &[&str] = if self.has_filter_query() {
            &["No matches", "Esc clears the filter."]
        } else {
            match self.active_tab_name().as_str() {
                "Local" => &["No local sources", "Open Settings to add folders."],
                "Playlists" => &["No playlists yet", "Create one from Playlists."],
                "Qobuz" => &["Use / to search Qobuz"],
                "Tidal" => &["Use / to search Tidal"],
                _ => &["No items"],
            }
        };
        lines
            .iter()
            .map(|line| ListItem::new(*line).style(theme::muted_style()))
            .collect()
    }

    fn next_tab(&mut self) {
        if !self.items.is_empty() {
            self.filter_input.exit_input_mode();
            self.selected_tab_index = (self.selected_tab_index + 1) % self.items.len();
            self.list_state.select(Some(0));
            self.update_cache();
            self.logger.debug(format!(
                "switched to {tab_name}",
                tab_name = self.items[self.selected_tab_index].name()
            ));
        }
    }

    fn previous_tab(&mut self) {
        if !self.items.is_empty() {
            self.filter_input.exit_input_mode();
            self.selected_tab_index = if self.selected_tab_index > 0 {
                self.selected_tab_index - 1
            } else {
                self.items.len() - 1
            };
            self.list_state.select(Some(0));
            self.update_cache();
            self.logger.debug(format!(
                "switched to {tab_name}",
                tab_name = self.items[self.selected_tab_index].name()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    use super::*;

    #[derive(Debug)]
    struct FakeSource {
        name: String,
        albums: Vec<String>,
    }

    impl FakeSource {
        fn new(name: &str, albums: &[&str]) -> Self {
            Self {
                name: name.to_string(),
                albums: albums.iter().map(|album| album.to_string()).collect(),
            }
        }
    }

    impl MusicSource for FakeSource {
        fn name(&self) -> String {
            self.name.clone()
        }

        fn get_albums(&self) -> Vec<String> {
            self.albums.clone()
        }

        fn get_album_path(&self, index: usize) -> Option<PathBuf> {
            self.albums
                .get(index)
                .map(|album| PathBuf::from(format!("/music/{album}")))
        }

        fn get_songs_from_album(&self, _path: PathBuf) -> Vec<Song> {
            Vec::new()
        }
    }

    fn extract_buffer_text(buffer: &Buffer) -> String {
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    text.push_str(cell.symbol());
                }
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn top_and_bottom_keys_jump_to_first_and_last_left_panel_items() {
        let (_log_panel, logger) = crate::ui::log_panel::LogPanel::new();
        let mut panel = LeftPanel::new(
            vec![Box::new(FakeSource::new(
                "Local",
                &["First", "Second", "Third"],
            ))],
            logger,
        );

        panel.handle_events(KeyEvent::from(KeyCode::End));
        assert_eq!(panel.selected_item_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        assert_eq!(panel.selected_item_index(), Some(0));

        panel.handle_events(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(panel.selected_item_index(), Some(2));

        panel.handle_events(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(panel.selected_item_index(), Some(0));
    }

    #[test]
    fn page_up_and_page_down_move_left_panel_selection_by_page() {
        let (_log_panel, logger) = crate::ui::log_panel::LogPanel::new();
        let albums: Vec<String> = (1..=12).map(|n| format!("Album {n}")).collect();
        let album_refs: Vec<&str> = albums.iter().map(String::as_str).collect();
        let mut panel = LeftPanel::new(
            vec![Box::new(FakeSource::new("Local", &album_refs))],
            logger,
        );

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.selected_item_index(), Some(10));

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.selected_item_index(), Some(11));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.selected_item_index(), Some(1));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.selected_item_index(), Some(0));
    }

    #[test]
    fn filtering_left_panel_items_preserves_source_indices() {
        let (_log_panel, logger) = crate::ui::log_panel::LogPanel::new();
        let mut panel = LeftPanel::new(
            vec![Box::new(FakeSource::new(
                "Local",
                &["Jazz Records", "Road Songs", "Sleep Sounds"],
            ))],
            logger,
        );

        panel.open_filter();
        for c in "sleep".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(c)));
        }

        assert_eq!(panel.selected_item_label().as_deref(), Some("Sleep Sounds"));
        assert_eq!(panel.selected_item_index(), Some(2));
        let (path, _songs) = panel
            .get_selected_album()
            .expect("filtered item should still open from source");
        assert_eq!(path, PathBuf::from("/music/Sleep Sounds"));
    }

    #[test]
    fn escape_clears_left_panel_filter() {
        let (_log_panel, logger) = crate::ui::log_panel::LogPanel::new();
        let mut panel = LeftPanel::new(
            vec![Box::new(FakeSource::new(
                "Local",
                &["Jazz Records", "Road Songs"],
            ))],
            logger,
        );

        panel.open_filter();
        for c in "road".chars() {
            panel.handle_events(KeyEvent::from(KeyCode::Char(c)));
        }
        panel.handle_events(KeyEvent::from(KeyCode::Esc));

        assert!(!panel.handles_escape());
        assert_eq!(panel.selected_item_index(), Some(0));
        assert_eq!(panel.selected_item_label().as_deref(), Some("Jazz Records"));
    }

    #[test]
    fn render_shows_scan_status_without_hiding_library_items() {
        let (_log_panel, logger) = crate::ui::log_panel::LogPanel::new();
        let mut panel = LeftPanel::new(
            vec![Box::new(FakeSource::new("Local", &["Jazz Records"]))],
            logger,
        );
        panel.set_status_line(Some("Indexing 1 source".to_string()));
        let backend = TestBackend::new(60, 12);
        let mut terminal = Terminal::new(backend).unwrap();

        let frame = terminal
            .draw(|frame| panel.render(frame, frame.area(), true))
            .unwrap();
        let text = extract_buffer_text(frame.buffer);

        assert!(text.contains("Scan:"));
        assert!(text.contains("Indexing 1 source"));
        assert!(text.contains("Jazz Records"));
    }
}
