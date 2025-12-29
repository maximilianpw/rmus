use std::sync::mpsc::{self, Receiver, Sender};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::{List, ListState};

use crate::ui::widget::list_from_strings;

struct LogItem {
    message: String,
}

#[derive(Clone)]
pub struct Logger {
    sender: Sender<LogItem>,
}

impl Logger {
    pub fn debug(&self, message: impl Into<String>) {
        let _ = self.sender.send(LogItem { message: message.into() });
    }
}

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
        while let Ok(item) = self.receiver.try_recv() {
            self.cached_items.push(format!("LOG: {}", &item.message));
            self.log_list.push(item);
        }
    }

    pub fn render(&mut self) -> List {
        list_from_strings(&self.cached_items, false)
    }

    pub(crate) fn handle_events(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(' ') => println!("test"),
            KeyCode::Char('d') => println!("delete"),
            _ => {}
        }
    }
}
