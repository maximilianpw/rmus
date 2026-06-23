use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};

use crate::{
    config::Config,
    ui::{
        settings::{account::AccountSettings, sources::SourceSettings, SettingsTab},
        theme,
        widget::handle_focused_border_style,
        AppPanel,
    },
    utils::centered_rect,
};

#[derive(Debug)]
pub struct SettingsPanel {
    pub opened: bool,
    selected_tab: usize,
    source_settings: SourceSettings,
    account_settings: AccountSettings,
}

impl AppPanel for SettingsPanel {
    fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, is_focused: bool) {
        if !self.opened {
            return;
        }

        let popup_area = centered_rect(80, 90, area);

        frame.render_widget(Clear, popup_area);

        let border_style = handle_focused_border_style(is_focused);
        let block = Block::bordered()
            .title(" Settings ")
            .border_style(border_style);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let [tabs_area, content_area] =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).areas(inner);

        self.render_tabs(frame, tabs_area);
        self.render_content(frame, content_area);
    }
}

impl SettingsPanel {
    pub fn new(config: Config) -> Self {
        let account_settings = AccountSettings::new(config.clone());
        Self {
            opened: false,
            selected_tab: 0,
            source_settings: SourceSettings::new(config),
            account_settings,
        }
    }

    pub fn toggle_open(&mut self) {
        self.opened = !self.opened;
    }

    pub fn select_tab(&mut self, tab: SettingsTab) {
        if let Some(index) = SettingsTab::ALL
            .iter()
            .position(|candidate| *candidate == tab)
        {
            self.selected_tab = index;
        }
    }

    /// Returns the updated config if any settings tab changed it.
    pub fn take_config_update(&mut self) -> Option<Config> {
        match (
            self.source_settings.take_config_update(),
            self.account_settings.take_config_update(),
        ) {
            (Some(mut source_config), Some(account_config)) => {
                source_config.qobuz = account_config.qobuz;
                source_config.tidal = account_config.tidal;
                Some(source_config)
            }
            (Some(source_config), None) => Some(source_config),
            (None, Some(account_config)) => Some(account_config),
            (None, None) => None,
        }
    }

    /// Push an updated config into the settings panel so its copy stays current.
    pub fn update_config(&mut self, config: &Config) {
        self.source_settings.update_config(config);
        self.account_settings.update_config(config);
    }

    pub fn take_tidal_clear_requested(&mut self) -> bool {
        self.account_settings.take_tidal_clear_requested()
    }

    pub fn take_qobuz_auth_requested(&mut self) -> bool {
        self.account_settings.take_qobuz_auth_requested()
    }

    pub fn set_qobuz_status_message(&mut self, text: Option<String>, is_error: bool) {
        self.account_settings
            .set_qobuz_status_message(text, is_error);
    }

    pub fn take_tidal_auth_requested(&mut self) -> bool {
        self.account_settings.take_tidal_auth_requested()
    }

    pub fn set_tidal_status_message(&mut self, text: Option<String>, is_error: bool) {
        self.account_settings
            .set_tidal_status_message(text, is_error);
    }

    pub fn close(&mut self) {
        self.opened = false;
    }

    pub fn is_input_active(&self) -> bool {
        match self.current_tab() {
            SettingsTab::General => self.source_settings.is_input_active(),
            SettingsTab::Account => self.account_settings.is_input_active(),
            SettingsTab::Keybinds => false,
        }
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        if !self.opened {
            return;
        }
        match self.current_tab() {
            SettingsTab::General if self.source_settings.handle_events(key) => {
                return;
            }
            SettingsTab::Account if self.account_settings.handle_events(key) => {
                return;
            }
            _ => {}
        }

        match key.code {
            KeyCode::Esc => {
                self.close();
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.next_tab();
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.prev_tab();
            }
            _ => {}
        }
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = (self.selected_tab + 1) % SettingsTab::ALL.len();
    }

    fn prev_tab(&mut self) {
        self.selected_tab = self
            .selected_tab
            .checked_sub(1)
            .unwrap_or(SettingsTab::ALL.len() - 1);
    }

    fn current_tab(&self) -> SettingsTab {
        SettingsTab::ALL[self.selected_tab]
    }

    fn render_tabs(&self, frame: &mut ratatui::Frame, area: Rect) {
        let titles: Vec<Line> = SettingsTab::ALL
            .iter()
            .map(|t| Line::from(t.title()))
            .collect();

        let tabs = Tabs::new(titles)
            .select(self.selected_tab)
            .highlight_style(theme::accent_bold_style())
            .divider(Span::raw(" │ "));

        frame.render_widget(tabs, area);
    }

    fn render_content(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let content_block = Block::default()
            .borders(Borders::TOP)
            .border_style(theme::muted_style());

        let inner = content_block.inner(area);
        frame.render_widget(content_block, area);

        match self.current_tab() {
            SettingsTab::General => self.source_settings.render_sources(frame, inner),
            SettingsTab::Account => self.account_settings.render(frame, inner),
            SettingsTab::Keybinds => self.render_keybinds(frame, inner),
        }
    }

    fn render_keybinds(&self, frame: &mut ratatui::Frame, area: Rect) {
        let keybind = |key: &str, desc: &str| -> Line<'static> {
            Line::from(vec![
                Span::styled(format!("{:<12}", key), theme::accent_style()),
                Span::raw(desc.to_string()),
            ])
        };

        let section = |title: &str| -> Line<'static> {
            Line::from(Span::styled(title.to_string(), theme::section_style()))
        };

        let left = vec![
            section("Global"),
            keybind("Esc", "Close active view / quit"),
            keybind("q", "Quit application"),
            keybind("Ctrl+C", "Quit application"),
            keybind("Tab", "Switch focused panel"),
            keybind("s", "Toggle settings"),
            keybind("?", "Open keybind help"),
            keybind("Q", "Show queue"),
            keybind("H", "Show recently played"),
            keybind("R", "Refresh library"),
            keybind("W", "Warm local metadata cache"),
            keybind("n / p", "Next / previous track"),
            keybind("+ / -", "Adjust volume"),
            keybind("m", "Mute / restore volume"),
            keybind("V", "Save current volume as startup"),
            keybind("z", "Toggle shuffle"),
            keybind("r", "Cycle repeat (Off/All/One)"),
            Line::from(""),
            section("Album List (Left Panel)"),
            keybind("Space/Enter", "Select album"),
            keybind("P", "Play album/playlist"),
            keybind("a", "Queue album/playlist"),
            keybind("F", "Add album/playlist to Favorites"),
            keybind("U", "Remove album/playlist from Favorites"),
            keybind("j / Down", "Move down"),
            keybind("k / Up", "Move up"),
            keybind("PageUp / PageDown", "Move by page"),
            keybind("Home / End", "Jump first / last"),
            keybind("f", "Filter Local/Playlists list"),
            keybind("C", "Create playlist (Playlists tab)"),
            keybind("E", "Rename playlist (Playlists tab)"),
            keybind("Y", "Duplicate playlist (Playlists tab)"),
            keybind("D", "Delete playlist (Playlists tab)"),
            Line::from(""),
            section("Song List (Center Panel)"),
            keybind("Space/Enter", "Play from selected song"),
            keybind("a", "Add to queue"),
            keybind("E", "Queue open collection"),
            keybind("A", "Add to playlist"),
            keybind("C", "Add collection to playlist"),
            keybind("F", "Add selected track to Favorites"),
            keybind("U", "Remove selected track from Favorites"),
            keybind("d", "Remove track from playlist"),
            keybind("J / K", "Move playlist track"),
            keybind("j / Down", "Move down"),
            keybind("k / Up", "Move up"),
            keybind("PageUp / PageDown", "Move by page"),
            keybind("Home / End", "Jump first / last"),
            Line::from(""),
            section("Search"),
            keybind("/", "Open search"),
            keybind("Tab", "Cycle search type"),
            keybind("Enter", "Execute search / play"),
            keybind("PageUp / PageDown", "Move by page"),
            keybind("Home / End", "Jump first / last result"),
            keybind("Esc", "Close search"),
        ];

        let right = vec![
            section("Playback (Right Panel)"),
            keybind("Space", "Toggle pause"),
            keybind("s", "Stop playback"),
            keybind("A", "Add current track to playlist"),
            keybind("F", "Add current track to Favorites"),
            keybind("U", "Remove current track from Favorites"),
            keybind("Left", "Seek backward 5s"),
            keybind("Right", "Seek forward 5s"),
            Line::from(""),
            section("Queue View"),
            keybind("Space/Enter", "Jump to queue track"),
            keybind("f", "Filter queue"),
            keybind("PageUp / PageDown", "Move by page"),
            keybind("Home / End", "Jump first / last"),
            keybind("A", "Add queue track to playlist"),
            keybind("F", "Add queue track to Favorites"),
            keybind("U", "Remove queue track from Favorites"),
            keybind("J / K", "Move queue track"),
            keybind("S", "Save queue as playlist"),
            keybind("d", "Remove queue track"),
            keybind("c", "Clear queued tracks"),
            keybind("Esc", "Close queue"),
            Line::from(""),
            section("History View"),
            keybind("Space/Enter", "Play history track"),
            keybind("f", "Filter history"),
            keybind("d", "Remove history track"),
            keybind("c", "Clear history"),
            keybind("Esc", "Close history"),
            Line::from(""),
            section("Logs Panel"),
            keybind("j / Down", "Move down"),
            keybind("k / Up", "Move up"),
            keybind("PageUp / PageDown", "Move by page"),
            keybind("Home / End", "Jump first / latest log"),
            keybind("h / Left", "Scroll log horizontally"),
            keybind("l / Right", "Scroll log horizontally"),
            keybind("c", "Clear logs"),
            Line::from(""),
            section("Text Inputs"),
            keybind("Left / Right", "Move cursor"),
            keybind("Home / End", "Jump cursor start / end"),
            keybind("Backspace/Delete", "Delete text"),
            Line::from(""),
            section("Settings Panel"),
            keybind("Esc", "Close settings"),
            keybind("Tab / l", "Next tab"),
            keybind("Shift+Tab / h", "Previous tab"),
            keybind("General: q", "Cycle max stream quality"),
            keybind("General: +/-", "Adjust startup volume"),
            keybind("General: z", "Toggle startup shuffle"),
            keybind("General: r", "Cycle startup repeat"),
            keybind("General: j/k", "Move source selection"),
            keybind("General: PageUp/PageDown", "Move sources by page"),
            keybind("General: Home/End", "Jump first / last source"),
            keybind("General: a", "Add local source"),
            keybind("General: e", "Edit local source"),
            keybind("General: d", "Remove local source"),
            keybind("Account: q", "Check Qobuz login"),
            keybind("Account: t", "Log in to Tidal"),
            keybind("Account: c", "Clear streaming accounts"),
        ];

        let [left_area, right_area] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(area);
        frame.render_widget(Paragraph::new(left), left_area);
        frame.render_widget(Paragraph::new(right), right_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("rmus-settings-panel-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pending_source_and_account_updates_are_merged() {
        let dir = temp_dir("merged-updates");
        let mut settings = SettingsPanel::new(Config::default());
        settings.toggle_open();

        settings.handle_events(key(KeyCode::Char('a')));
        for c in "Library".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in dir.to_string_lossy().chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        settings.handle_events(key(KeyCode::Tab));
        settings.handle_events(key(KeyCode::Char('e')));
        for c in "user@example.com".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Tab));
        for c in "secret".chars() {
            settings.handle_events(key(KeyCode::Char(c)));
        }
        settings.handle_events(key(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("pending settings changes should produce one merged update");
        assert_eq!(updated.local.sources.len(), 1);
        assert_eq!(updated.local.sources[0].name, "Library");
        assert_eq!(updated.local.sources[0].path, dir.canonicalize().unwrap());
        let qobuz = updated.qobuz.expect("qobuz account should be included");
        assert_eq!(qobuz.email, "user@example.com");
        assert_eq!(qobuz.password, "secret");
        assert!(
            settings.take_config_update().is_none(),
            "merged tab updates should be consumed together"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
