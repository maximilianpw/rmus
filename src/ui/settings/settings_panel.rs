use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};

use crate::{
    config::Config,
    ui::{
        settings::{account::AccountSettings, sources::SourceSettings, SettingsTab},
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

        let popup_area = centered_rect(60, 70, area);

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

    /// Returns the updated config if account settings changed it.
    pub fn take_config_update(&mut self) -> Option<Config> {
        self.account_settings.take_config_update()
    }

    /// Push an updated config into the settings panel so its copy stays current.
    pub fn update_config(&mut self, config: &Config) {
        self.account_settings.update_config(config);
    }

    pub fn close(&mut self) {
        self.opened = false;
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        if !self.opened {
            return;
        }
        match self.current_tab() {
            SettingsTab::General => {
                if self.source_settings.handle_events(key) {
                    return;
                }
            }
            SettingsTab::Account => {
                if self.account_settings.handle_events(key) {
                    return;
                }
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
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .divider(Span::raw(" │ "));

        frame.render_widget(tabs, area);
    }

    fn render_content(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let content_block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray));

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
                Span::styled(format!("{:<12}", key), Style::default().fg(Color::Cyan)),
                Span::raw(desc.to_string()),
            ])
        };

        let section = |title: &str| -> Line<'static> {
            Line::from(Span::styled(
                title.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
        };

        let text = vec![
            section("Global"),
            keybind("Esc / q", "Quit application"),
            keybind("Ctrl+C", "Quit application"),
            keybind("Tab", "Switch focused panel"),
            keybind("s", "Toggle settings"),
            Line::from(""),
            section("Album List (Left Panel)"),
            keybind("Space", "Select album"),
            keybind("j / Down", "Move down"),
            keybind("k / Up", "Move up"),
            Line::from(""),
            section("Song List (Center Panel)"),
            keybind("Space", "Play from selected song"),
            keybind("j / Down", "Move down"),
            keybind("k / Up", "Move up"),
            Line::from(""),
            section("Search"),
            keybind("/", "Open search"),
            keybind("Enter", "Execute search / play"),
            keybind("Esc", "Close search"),
            Line::from(""),
            section("Playback (Right Panel)"),
            keybind("Space", "Toggle pause"),
            keybind("n", "Next track"),
            keybind("p", "Previous track"),
            keybind("s", "Stop playback"),
            keybind("+ / =", "Volume up"),
            keybind("-", "Volume down"),
            keybind("Left", "Seek backward 5s"),
            keybind("Right", "Seek forward 5s"),
            Line::from(""),
            section("Settings Panel"),
            keybind("Esc", "Close settings"),
            keybind("Tab / l", "Next tab"),
            keybind("Shift+Tab / h", "Previous tab"),
        ];

        let paragraph = Paragraph::new(text);
        frame.render_widget(paragraph, area);
    }
}
