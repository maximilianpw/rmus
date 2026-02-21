use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{config::Config, ui::input_line::InputLine};

#[derive(Debug)]
pub struct AccountSettings {
    config: Config,
    input_mode: bool,
    email_input: InputLine,
    password_input: InputLine,
    active_field: usize, // 0 = email, 1 = password
}

impl AccountSettings {
    pub fn new(config: Config) -> Self {
        let (email, password) = config
            .qobuz
            .as_ref()
            .map(|q| (q.email.clone(), q.password.clone()))
            .unwrap_or_default();

        let mut email_input = InputLine::new();
        email_input.value = email;

        let mut password_input = InputLine::new();
        password_input.value = password;

        Self {
            config,
            input_mode: false,
            email_input,
            password_input,
            active_field: 0,
        }
    }

    pub fn render(&self, frame: &mut ratatui::Frame, area: Rect) {
        let [email_area, password_area, hint_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .areas(area);

        let label_style = Style::default().fg(Color::Cyan);
        let active_label_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        // Email field
        let email_label_style = if self.input_mode && self.active_field == 0 {
            active_label_style
        } else {
            label_style
        };
        let email_cursor = if self.input_mode && self.active_field == 0 {
            "_"
        } else {
            ""
        };
        let email_line = Line::from(vec![
            Span::styled("Email:    ", email_label_style),
            Span::raw(format!("{}{}", self.email_input.value, email_cursor)),
        ]);
        frame.render_widget(Paragraph::new(email_line), email_area);

        // Password field
        let password_label_style = if self.input_mode && self.active_field == 1 {
            active_label_style
        } else {
            label_style
        };
        let password_cursor = if self.input_mode && self.active_field == 1 {
            "_"
        } else {
            ""
        };
        let display_password = if self.input_mode {
            self.password_input.value.clone()
        } else {
            "*".repeat(self.password_input.value.len())
        };
        let password_line = Line::from(vec![
            Span::styled("Password: ", password_label_style),
            Span::raw(format!("{}{}", display_password, password_cursor)),
        ]);
        frame.render_widget(Paragraph::new(password_line), password_area);

        // Hint
        let hint = if self.input_mode {
            Line::from(
                "Tab: switch field | Enter: save | Esc: cancel"
                    .fg(Color::DarkGray),
            )
        } else {
            Line::from("e: edit account".fg(Color::DarkGray))
        };
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
                    self.email_input.value = email;
                    self.password_input.value = password;
                    self.input_mode = false;
                }
                KeyCode::Enter => {
                    self.email_input.confirm_input();
                    self.password_input.confirm_input();
                    self.input_mode = false;
                    self.save_to_config();
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
                KeyCode::Char('e') => {
                    self.input_mode = true;
                    self.email_input.enter_input_mode();
                    self.password_input.enter_input_mode();
                    // Preserve existing values
                    let (email, password) = self
                        .config
                        .qobuz
                        .as_ref()
                        .map(|q| (q.email.clone(), q.password.clone()))
                        .unwrap_or_default();
                    self.email_input.value = email;
                    self.email_input.active = true;
                    self.password_input.value = password;
                    self.password_input.active = true;
                    self.active_field = 0;
                    true
                }
                _ => false,
            }
        }
    }

    fn save_to_config(&mut self) {
        let email = self.email_input.value.clone();
        let password = self.password_input.value.clone();

        if let Some(ref mut qobuz) = self.config.qobuz {
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

        let _ = self.config.save();
    }
}
