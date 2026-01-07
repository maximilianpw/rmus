use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::ui::widget::handle_focused_border_style;

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
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, is_focused: bool) {
        let border_style = handle_focused_border_style(is_focused);
        let selected = self.list_state.selected();

        let items: Vec<ListItem> = self
            .log_list
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let (prefix, color) = match item.level {
                    LogLevel::Info => ("INFO: ", Color::Blue),
                    LogLevel::Debug => ("DEBUG: ", Color::Yellow),
                    LogLevel::Error => ("ERROR: ", Color::Red),
                };
                let msg = if selected == Some(i) && self.h_scroll > 0 {
                    let skip = self.h_scroll.min(item.message.len());
                    &item.message[skip..]
                } else {
                    &item.message
                };
                let line = Line::from(vec![
                    Span::styled(prefix, Style::default().fg(color)),
                    Span::raw(msg),
                ]);
                ListItem::new(line)
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title("Logs")
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(Style::default().bg(Color::DarkGray));
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    pub fn scroll_down(&mut self) {
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
                Some(item) if self.h_scroll < item.message.len() => {
                    self.h_scroll += 1;
                }
                _ => (),
            }
        }
    }

    pub fn scroll_left(&mut self) {
        self.h_scroll = self.h_scroll.saturating_sub(1);
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll_down(),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_up(),
            KeyCode::Right | KeyCode::Char('l') => self.scroll_right(),
            KeyCode::Left | KeyCode::Char('h') => self.scroll_left(),
            _ => {}
        }
    }
}
