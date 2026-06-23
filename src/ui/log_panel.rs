use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::ui::{
    theme,
    widget::{handle_focused_border_style, selected_row_style},
    AppPanel,
};

const PAGE_STEP: usize = 10;

#[derive(Debug)]
pub enum LogLevel {
    Info,
    Debug,
    Error,
}

#[derive(Debug)]
pub struct LogItem {
    level: LogLevel,
    message: String,
}

#[derive(Debug, Clone)]
pub struct Logger {
    sender: Sender<LogItem>,
}

impl Logger {
    pub fn debug(&self, message: impl Into<String>) {
        let _ = self.sender.send(LogItem {
            level: LogLevel::Debug,
            message: message.into(),
        });
    }

    pub fn error(&self, message: impl Into<String>) {
        let _ = self.sender.send(LogItem {
            level: LogLevel::Error,
            message: message.into(),
        });
    }

    pub fn info(&self, message: impl Into<String>) {
        let _ = self.sender.send(LogItem {
            level: LogLevel::Info,
            message: message.into(),
        });
    }
}

#[derive(Debug)]
pub struct LogPanel {
    receiver: Receiver<LogItem>,
    log_list: Vec<LogItem>,
    list_state: ListState,
    h_scroll: usize,
}

impl AppPanel for LogPanel {
    fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = handle_focused_border_style(is_focused);
        let selected = self.list_state.selected();

        let items: Vec<ListItem> = if self.log_list.is_empty() {
            vec![ListItem::new("No logs yet").style(theme::muted_style())]
        } else {
            self.log_list
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let (prefix, color) = match item.level {
                        LogLevel::Info => ("INFO: ", theme::INFO),
                        LogLevel::Debug => ("DEBUG: ", theme::WARNING),
                        LogLevel::Error => ("ERROR: ", theme::ERROR),
                    };
                    let msg = if selected == Some(i) && self.h_scroll > 0 {
                        Self::message_from_scroll(&item.message, self.h_scroll)
                    } else {
                        &item.message
                    };
                    let line = Line::from(vec![
                        Span::styled(prefix, theme::default_style().fg(color)),
                        Span::raw(msg),
                    ]);
                    ListItem::new(line)
                })
                .collect()
        };
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title("Logs")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(selected_row_style());
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }
}
impl LogPanel {
    pub fn new() -> (Self, Logger) {
        let (sender, receiver) = mpsc::channel();
        let panel = Self {
            receiver,
            log_list: Vec::new(),
            list_state: ListState::default(),
            h_scroll: 0,
        };
        (panel, Logger { sender })
    }

    /// Call this each frame to drain pending log messages
    pub fn poll(&mut self) {
        let mut new_items = false;
        while let Ok(item) = self.receiver.try_recv() {
            self.log_list.push(item);
            new_items = true;
        }
        if new_items {
            self.list_state
                .select(Some(self.log_list.len().saturating_sub(1)));
            self.h_scroll = 0;
        }
    }

    pub fn scroll_down(&mut self) {
        if self.log_list.is_empty() {
            self.list_state.select(None);
            self.h_scroll = 0;
            return;
        }

        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.log_list.len().saturating_sub(1) {
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

    pub fn scroll_up(&mut self) {
        if self.log_list.is_empty() {
            self.list_state.select(None);
            self.h_scroll = 0;
            return;
        }

        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.log_list.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.h_scroll = 0;
    }

    pub fn scroll_right(&mut self) {
        if let Some(i) = self.list_state.selected() {
            match self.log_list.get(i) {
                Some(item) if self.h_scroll < item.message.chars().count() => {
                    self.h_scroll += 1;
                }
                _ => (),
            }
        }
    }

    pub fn scroll_left(&mut self) {
        self.h_scroll = self.h_scroll.saturating_sub(1);
    }

    pub fn page_down(&mut self) {
        if !self.log_list.is_empty() {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state.select(Some(
                current
                    .saturating_add(PAGE_STEP)
                    .min(self.log_list.len() - 1),
            ));
            self.h_scroll = 0;
        }
    }

    pub fn page_up(&mut self) {
        if !self.log_list.is_empty() {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state
                .select(Some(current.saturating_sub(PAGE_STEP)));
            self.h_scroll = 0;
        }
    }

    pub fn first(&mut self) {
        if !self.log_list.is_empty() {
            self.list_state.select(Some(0));
            self.h_scroll = 0;
        }
    }

    pub fn last(&mut self) {
        if !self.log_list.is_empty() {
            self.list_state.select(Some(self.log_list.len() - 1));
            self.h_scroll = 0;
        }
    }

    pub fn clear(&mut self) {
        self.log_list.clear();
        self.list_state.select(None);
        self.h_scroll = 0;
    }

    #[cfg(test)]
    pub(crate) fn messages(&self) -> Vec<&str> {
        self.log_list
            .iter()
            .map(|item| item.message.as_str())
            .collect()
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll_down(),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::Home => self.first(),
            KeyCode::End => self.last(),
            KeyCode::Right | KeyCode::Char('l') => self.scroll_right(),
            KeyCode::Left | KeyCode::Char('h') => self.scroll_left(),
            KeyCode::Char('c') => self.clear(),
            _ => {}
        }
    }

    fn message_from_scroll(message: &str, h_scroll: usize) -> &str {
        if h_scroll == 0 {
            return message;
        }

        message
            .char_indices()
            .nth(h_scroll)
            .map_or("", |(index, _)| &message[index..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn horizontal_scroll_handles_non_ascii_messages() {
        let (mut panel, logger) = LogPanel::new();
        logger.info("éclair stream ready");
        panel.poll();

        panel.scroll_right();

        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area(), true))
            .unwrap();
    }

    #[test]
    fn new_log_messages_reset_horizontal_scroll() {
        let (mut panel, logger) = LogPanel::new();
        logger.info("first long message");
        panel.poll();
        panel.scroll_right();

        assert_eq!(panel.h_scroll, 1);

        logger.info("second message");
        panel.poll();

        assert_eq!(panel.list_state.selected(), Some(1));
        assert_eq!(panel.h_scroll, 0);
    }

    #[test]
    fn page_up_and_page_down_move_log_selection_by_page() {
        let (mut panel, logger) = LogPanel::new();
        for n in 1..=12 {
            logger.info(format!("message {n}"));
        }
        panel.poll();

        assert_eq!(panel.list_state.selected(), Some(11));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.list_state.selected(), Some(1));

        panel.handle_events(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(panel.list_state.selected(), Some(0));

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.list_state.selected(), Some(10));

        panel.handle_events(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(panel.list_state.selected(), Some(11));
    }

    #[test]
    fn home_and_end_jump_to_first_and_last_log_entries() {
        let (mut panel, logger) = LogPanel::new();
        for n in 1..=3 {
            logger.info(format!("message {n}"));
        }
        panel.poll();
        panel.scroll_right();

        panel.handle_events(KeyEvent::from(KeyCode::Home));
        assert_eq!(panel.list_state.selected(), Some(0));
        assert_eq!(panel.h_scroll, 0);

        panel.scroll_right();
        panel.handle_events(KeyEvent::from(KeyCode::End));
        assert_eq!(panel.list_state.selected(), Some(2));
        assert_eq!(panel.h_scroll, 0);
    }

    #[test]
    fn empty_log_navigation_keeps_selection_empty() {
        let (mut panel, _logger) = LogPanel::new();

        panel.handle_events(KeyEvent::from(KeyCode::Down));
        assert_eq!(panel.list_state.selected(), None);

        panel.handle_events(KeyEvent::from(KeyCode::Up));
        assert_eq!(panel.list_state.selected(), None);
    }

    #[test]
    fn clear_removes_logs_and_resets_selection() {
        let (mut panel, logger) = LogPanel::new();
        logger.info("first message");
        logger.error("second message");
        panel.poll();
        panel.scroll_right();

        panel.handle_events(KeyEvent::from(KeyCode::Char('c')));

        assert!(panel.log_list.is_empty());
        assert_eq!(panel.list_state.selected(), None);
        assert_eq!(panel.h_scroll, 0);
    }
}
