use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
};

use crate::ui::{center_panel::CenterPanel, left_panel::LeftPanel, right_panel::RightPanel};

mod ui;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = App::new().run(terminal);
    ratatui::restore();
    result
}

#[derive(Debug, Default)]
pub struct App {
    running: bool,
    focused_window: FocusedWindow,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum FocusedWindow {
    #[default]
    Left,
    Center,
    Right,
}

impl FocusedWindow {
    fn next(&self) -> Self {
        match self {
            Self::Left => Self::Center,
            Self::Center => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        self.running = true;
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_crossterm_events()?;
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Percentage(60),
            Constraint::Fill(1),
        ]);
        let [left_area, center_area, right_area] = layout.areas(frame.area());

        LeftPanel::new().render(frame, left_area, self.focused_window == FocusedWindow::Left);
        CenterPanel::new().render(
            frame,
            center_area,
            self.focused_window == FocusedWindow::Center,
        );
        RightPanel::new().render(
            frame,
            right_area,
            self.focused_window == FocusedWindow::Right,
        );
    }

    fn handle_crossterm_events(&mut self) -> color_eyre::Result<()> {
        match event::read()? {
            // it's important to check KeyEventKind::Press to avoid handling key release events
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            _ => {}
        }
        Ok(())
    }

    fn on_key_event(&mut self, key: KeyEvent) {
        // Global key events first
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => {
                self.quit();
                return;
            }
            (_, KeyCode::Tab) => {
                self.focused_window = self.focused_window.next();
                return;
            }
            _ => {} // Continue to focused window specific events
        }

        // Focused window specific key events
        match self.focused_window {
            FocusedWindow::Left => self.handle_left_panel_events(key),
            FocusedWindow::Center => self.handle_center_panel_events(key),
            FocusedWindow::Right => self.handle_right_panel_events(key),
        }
    }

    fn handle_left_panel_events(&mut self, key: KeyEvent) {
        // Example: Handle 'j' and 'k' for navigation in the left panel
        match key.code {
            KeyCode::Char('j') => {
                // Logic for moving down in left panel's list/selection
                // println!("Left Panel: Move Down (j)"); // For debugging
            }
            KeyCode::Char('k') => {
                // Logic for moving up in left panel's list/selection
                // println!("Left Panel: Move Up (k)"); // For debugging
            }
            _ => {}
        }
    }

    fn handle_center_panel_events(&mut self, key: KeyEvent) {
        // Example: Handle 'space' for play/pause in the center panel
        match key.code {
            KeyCode::Char(' ') => {
                // Logic for playing/pausing music
                // println!("Center Panel: Play/Pause (Space)"); // For debugging
            }
            _ => {}
        }
    }

    fn handle_right_panel_events(&mut self, key: KeyEvent) {
        // Add specific key events for the right panel here
        match key.code {
            _ => {}
        }
    }

    fn quit(&mut self) {
        self.running = false;
    }
}
