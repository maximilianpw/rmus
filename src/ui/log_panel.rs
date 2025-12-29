use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::sync::mpsc::{self, Receiver, Sender};

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
    cached_items: Vec<String>,
    list_state: ListState,
}

impl LogPanel {
    pub fn new() -> (Self, Logger) {
        let (sender, receiver) = mpsc::channel();
        let panel = Self {
            receiver,
            log_list: Vec::new(),
            cached_items: Vec::new(),
            list_state: ListState::default(),
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

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .log_list
            .iter()
            .map(|item| {
                let (prefix, color) = match item.level {
                    LogLevel::Info => ("INFO: ", Color::Blue),
                    LogLevel::Debug => ("DEBUG: ", Color::Yellow),
                    LogLevel::Error => ("ERROR: ", Color::Red),
                };
                let line = Line::from(vec![
                    Span::styled(prefix, Style::default().fg(color)),
                    Span::raw(&item.message),
                ]);
                ListItem::new(line)
            })
            .collect();
        let list = List::new(items)
            .block(Block::bordered().title("Logs").borders(Borders::ALL))
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
    }

    pub fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.scroll_down(),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_up(),
            _ => {}
        }
    }
}
