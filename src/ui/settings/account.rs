use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    config::Config,
    ui::{input_line::InputLine, theme},
};

#[derive(Debug)]
pub struct AccountSettings {
    config: Config,
    input_mode: bool,
    email_input: InputLine,
    password_input: InputLine,
    active_field: usize, // 0 = email, 1 = password
    config_dirty: bool,
    tidal_clear_requested: bool,
    qobuz_auth_requested: bool,
    tidal_auth_requested: bool,
    status_message: Option<AccountStatusMessage>,
    tidal_status_message: Option<AccountStatusMessage>,
}

#[derive(Debug)]
struct AccountStatusMessage {
    text: String,
    is_error: bool,
}

impl AccountSettings {
    pub fn new(config: Config) -> Self {
        let (email, password) = config
            .qobuz
            .as_ref()
            .map(|q| (q.email.clone(), q.password.clone()))
            .unwrap_or_default();

        let mut email_input = InputLine::new();
        email_input.set_value(email);

        let mut password_input = InputLine::new();
        password_input.set_value(password);

        Self {
            config,
            input_mode: false,
            email_input,
            password_input,
            active_field: 0,
            config_dirty: false,
            tidal_clear_requested: false,
            qobuz_auth_requested: false,
            tidal_auth_requested: false,
            status_message: None,
            tidal_status_message: None,
        }
    }

    pub fn render(&self, frame: &mut ratatui::Frame, area: Rect) {
        let [qobuz_area, tidal_area] =
            Layout::vertical([Constraint::Length(9), Constraint::Fill(1)]).areas(area);

        self.render_qobuz_section(frame, qobuz_area);
        self.render_tidal_section(frame, tidal_area);
    }

    fn render_qobuz_section(&self, frame: &mut ratatui::Frame, area: Rect) {
        let [header_area, status_area, email_area, password_area, message_area, hint_area] =
            Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(area);

        // Section header
        let header = Line::from(Span::styled("── Qobuz ──", theme::section_style()));
        frame.render_widget(Paragraph::new(header), header_area);

        let status_text = if self.has_qobuz_credentials() {
            Line::from(vec![
                Span::styled("Status:   ", theme::accent_style()),
                Span::styled("Configured", theme::success_style()),
            ])
        } else {
            Line::from(vec![
                Span::styled("Status:   ", theme::accent_style()),
                Span::styled("Not configured", theme::error_style()),
            ])
        };
        frame.render_widget(Paragraph::new(status_text), status_area);

        let label_style = theme::accent_style();
        let active_label_style = theme::accent_bold_style();

        // Email field
        let email_label_style = if self.input_mode && self.active_field == 0 {
            active_label_style
        } else {
            label_style
        };
        let mut email_spans = vec![Span::styled("Email:    ", email_label_style)];
        email_spans.extend(self.email_input.display_spans(
            self.input_mode && self.active_field == 0,
            theme::accent_style(),
        ));
        let email_line = Line::from(email_spans);
        frame.render_widget(Paragraph::new(email_line), email_area);

        // Password field
        let password_label_style = if self.input_mode && self.active_field == 1 {
            active_label_style
        } else {
            label_style
        };
        let mut password_spans = vec![Span::styled("Password: ", password_label_style)];
        if self.input_mode && self.active_field == 1 {
            password_spans.extend(
                self.password_input
                    .display_spans(true, theme::accent_style()),
            );
        } else {
            password_spans.push(Span::raw("*".repeat(self.password_input.value.len())));
        }
        let password_line = Line::from(password_spans);
        frame.render_widget(Paragraph::new(password_line), password_area);

        let message = self
            .status_message
            .as_ref()
            .map(|message| {
                let color = if message.is_error {
                    theme::ERROR
                } else {
                    theme::SUCCESS
                };
                Line::from(message.text.clone().fg(color))
            })
            .unwrap_or_else(|| Line::from(""));
        frame.render_widget(Paragraph::new(message), message_area);

        // Hint
        let hint = if self.input_mode {
            Line::from("Tab: switch field | Enter: save | Esc: cancel".fg(theme::MUTED))
        } else {
            Line::from("e: edit account | c: clear accounts".fg(theme::MUTED))
        };
        frame.render_widget(Paragraph::new(hint), hint_area);
    }

    fn has_qobuz_credentials(&self) -> bool {
        self.config
            .qobuz
            .as_ref()
            .map(crate::config::QobuzConfig::has_credentials)
            .unwrap_or(false)
    }

    fn render_tidal_section(&self, frame: &mut ratatui::Frame, area: Rect) {
        let [header_area, status_area, message_area, hint_area] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .areas(area);

        // Section header
        let header = Line::from(Span::styled("── Tidal ──", theme::section_style()));
        frame.render_widget(Paragraph::new(header), header_area);

        let has_token = self
            .config
            .tidal
            .as_ref()
            .map(crate::config::TidalConfig::has_access_token)
            .unwrap_or(false);

        let status_text = if has_token {
            Line::from(vec![
                Span::styled("Status:   ", theme::accent_style()),
                Span::styled("Authenticated", theme::success_style()),
            ])
        } else {
            Line::from(vec![
                Span::styled("Status:   ", theme::accent_style()),
                Span::styled("Not authenticated", theme::error_style()),
            ])
        };
        frame.render_widget(Paragraph::new(status_text), status_area);

        let message = self
            .tidal_status_message
            .as_ref()
            .map(|message| {
                let color = if message.is_error {
                    theme::ERROR
                } else {
                    theme::SUCCESS
                };
                Line::from(message.text.clone().fg(color))
            })
            .unwrap_or_else(|| Line::from(""));
        frame.render_widget(Paragraph::new(message), message_area);

        let hint_text = if has_token {
            "t: reauthenticate Tidal | c: clear accounts"
        } else {
            "t: log in to Tidal | c: clear accounts"
        };
        let hint = Line::from(hint_text.fg(theme::MUTED));
        frame.render_widget(Paragraph::new(hint), hint_area);
    }

    pub fn handle_events(&mut self, key: KeyEvent) -> bool {
        if self.input_mode {
            let active_input = if self.active_field == 0 {
                &mut self.email_input
            } else {
                &mut self.password_input
            };

            match key.code {
                KeyCode::Esc => {
                    // Restore original values from config
                    let (email, password) = self
                        .config
                        .qobuz
                        .as_ref()
                        .map(|q| (q.email.clone(), q.password.clone()))
                        .unwrap_or_default();
                    self.email_input.exit_input_mode();
                    self.password_input.exit_input_mode();
                    self.email_input.set_value(email);
                    self.password_input.set_value(password);
                    self.input_mode = false;
                }
                KeyCode::Enter => {
                    self.email_input.confirm_input();
                    self.password_input.confirm_input();
                    self.input_mode = false;
                    self.save_qobuz_to_config();
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
                KeyCode::Delete => {
                    active_input.delete_next_char();
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
                KeyCode::Char('e') => {
                    self.input_mode = true;
                    self.status_message = None;
                    self.email_input.enter_input_mode();
                    self.password_input.enter_input_mode();
                    let (email, password) = self
                        .config
                        .qobuz
                        .as_ref()
                        .map(|q| (q.email.clone(), q.password.clone()))
                        .unwrap_or_default();
                    self.email_input.set_value(email);
                    self.email_input.active = true;
                    self.password_input.set_value(password);
                    self.password_input.active = true;
                    self.active_field = 0;
                    true
                }
                KeyCode::Char('c') => {
                    self.clear_accounts();
                    true
                }
                KeyCode::Char('q') => {
                    self.qobuz_auth_requested = true;
                    self.status_message = Some(AccountStatusMessage {
                        text: "Checking Qobuz account...".to_string(),
                        is_error: false,
                    });
                    true
                }
                KeyCode::Char('t') => {
                    self.tidal_auth_requested = true;
                    self.tidal_status_message = Some(AccountStatusMessage {
                        text: "Starting Tidal login...".to_string(),
                        is_error: false,
                    });
                    true
                }
                _ => false,
            }
        }
    }

    pub fn is_input_active(&self) -> bool {
        self.input_mode
    }

    fn save_qobuz_to_config(&mut self) {
        let email = self.email_input.value.trim().to_string();
        let password = self.password_input.value.clone();

        let success_message = if email.is_empty() || password.trim().is_empty() {
            "Qobuz account cleared"
        } else {
            "Qobuz account saved"
        };

        if email.is_empty() || password.trim().is_empty() {
            self.config.qobuz = None;
        } else if let Some(ref mut qobuz) = self.config.qobuz {
            qobuz.email = email;
            qobuz.password = password;
        } else {
            self.config.qobuz = Some(crate::config::QobuzConfig {
                email,
                password,
                app_id: String::new(),
                app_secret: String::new(),
            });
        }

        self.config_dirty = true;
        self.status_message = match self.config.save() {
            Ok(()) => Some(AccountStatusMessage {
                text: success_message.to_string(),
                is_error: false,
            }),
            Err(_) => Some(AccountStatusMessage {
                text: "Failed to save account".to_string(),
                is_error: true,
            }),
        };
    }

    fn clear_accounts(&mut self) {
        self.config.qobuz = None;
        self.config.tidal = None;
        self.email_input.value.clear();
        self.password_input.value.clear();
        self.input_mode = false;
        self.config_dirty = true;
        self.tidal_clear_requested = true;
        self.qobuz_auth_requested = false;
        self.tidal_auth_requested = false;
        self.tidal_status_message = None;
        self.status_message = match self.config.save() {
            Ok(()) => Some(AccountStatusMessage {
                text: "Streaming accounts cleared".to_string(),
                is_error: false,
            }),
            Err(_) => Some(AccountStatusMessage {
                text: "Failed to clear accounts".to_string(),
                is_error: true,
            }),
        };
    }

    pub fn take_tidal_clear_requested(&mut self) -> bool {
        let requested = self.tidal_clear_requested;
        self.tidal_clear_requested = false;
        requested
    }

    pub fn take_qobuz_auth_requested(&mut self) -> bool {
        let requested = self.qobuz_auth_requested;
        self.qobuz_auth_requested = false;
        requested
    }

    pub fn set_qobuz_status_message(&mut self, text: Option<String>, is_error: bool) {
        self.status_message = text.map(|text| AccountStatusMessage { text, is_error });
    }

    pub fn take_tidal_auth_requested(&mut self) -> bool {
        let requested = self.tidal_auth_requested;
        self.tidal_auth_requested = false;
        requested
    }

    pub fn set_tidal_status_message(&mut self, text: Option<String>, is_error: bool) {
        self.tidal_status_message = text.map(|text| AccountStatusMessage { text, is_error });
    }

    /// Returns the updated config if it changed since last check.
    pub fn take_config_update(&mut self) -> Option<Config> {
        if self.config_dirty {
            self.config_dirty = false;
            Some(self.config.clone())
        } else {
            None
        }
    }

    /// Update the internal config copy to stay in sync with the App's config.
    pub fn update_config(&mut self, config: &Config) {
        self.config = config.clone();
        if !self.input_mode {
            let (email, password) = self
                .config
                .qobuz
                .as_ref()
                .map(|q| (q.email.clone(), q.password.clone()))
                .unwrap_or_default();
            self.email_input.set_value(email);
            self.password_input.set_value(password);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{QobuzConfig, TidalConfig};

    #[test]
    fn update_config_refreshes_displayed_qobuz_credentials_when_not_editing() {
        let mut settings = AccountSettings::new(Config::default());
        let config = Config {
            qobuz: Some(QobuzConfig {
                email: "listener@example.com".to_string(),
                password: "secret".to_string(),
                app_id: "app".to_string(),
                app_secret: "secret".to_string(),
            }),
            tidal: Some(TidalConfig {
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                country_code: "US".to_string(),
                token_expiry: 1_900_000_000,
            }),
            ..Default::default()
        };

        settings.update_config(&config);

        assert_eq!(settings.email_input.value, "listener@example.com");
        assert_eq!(settings.password_input.value, "secret");
        assert!(settings.has_qobuz_credentials());
    }

    #[test]
    fn tidal_login_request_is_consumed_once() {
        let mut settings = AccountSettings::new(Config::default());

        assert!(settings.handle_events(KeyEvent::from(KeyCode::Char('t'))));

        assert!(settings.take_tidal_auth_requested());
        assert!(!settings.take_tidal_auth_requested());
        assert_eq!(
            settings
                .tidal_status_message
                .as_ref()
                .map(|message| message.text.as_str()),
            Some("Starting Tidal login...")
        );
    }

    #[test]
    fn qobuz_login_request_is_consumed_once() {
        let mut settings = AccountSettings::new(Config::default());

        assert!(settings.handle_events(KeyEvent::from(KeyCode::Char('q'))));

        assert!(settings.take_qobuz_auth_requested());
        assert!(!settings.take_qobuz_auth_requested());
        assert_eq!(
            settings
                .status_message
                .as_ref()
                .map(|message| message.text.as_str()),
            Some("Checking Qobuz account...")
        );
    }

    #[test]
    fn account_text_inputs_support_home_end_and_delete() {
        let mut settings = AccountSettings::new(Config::default());

        assert!(settings.handle_events(KeyEvent::from(KeyCode::Char('e'))));
        for c in "Xlistener".chars() {
            settings.handle_events(KeyEvent::from(KeyCode::Char(c)));
        }
        settings.handle_events(KeyEvent::from(KeyCode::Home));
        settings.handle_events(KeyEvent::from(KeyCode::Delete));
        settings.handle_events(KeyEvent::from(KeyCode::End));
        for c in "@example.com".chars() {
            settings.handle_events(KeyEvent::from(KeyCode::Char(c)));
        }

        settings.handle_events(KeyEvent::from(KeyCode::Tab));
        for c in "Xsecret".chars() {
            settings.handle_events(KeyEvent::from(KeyCode::Char(c)));
        }
        settings.handle_events(KeyEvent::from(KeyCode::Home));
        settings.handle_events(KeyEvent::from(KeyCode::Delete));
        settings.handle_events(KeyEvent::from(KeyCode::Enter));

        let updated = settings
            .take_config_update()
            .expect("editing account should produce config update");
        let qobuz = updated.qobuz.expect("qobuz account should be configured");
        assert_eq!(qobuz.email, "listener@example.com");
        assert_eq!(qobuz.password, "secret");
    }
}
