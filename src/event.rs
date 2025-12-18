use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, FocusedWindow};

pub fn handle_crossterm_events(app: &mut App) -> color_eyre::Result<()> {
    match event::read()? {
        // it's important to check KeyEventKind::Press to avoid handling key release events
        Event::Key(key) if key.kind == KeyEventKind::Press => on_key_event(app, key),
        Event::Mouse(_) => {}
        Event::Resize(_, _) => {}
        _ => {}
    }
    Ok(())
}

pub fn on_key_event(app: &mut App, key: KeyEvent) {
    // Global key events first
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc | KeyCode::Char('q'))
        | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => {
            app.quit();
            return;
        }
        (_, KeyCode::Tab) => {
            app.focused_window = app.focused_window.next();
            return;
        }
        _ => {} // Continue to focused window specific events
    }

    // Focused window specific key events
    match app.focused_window {
        FocusedWindow::Left => app.left_panel.handle_events(key),
        FocusedWindow::Center => app.center_panel.handle_events(key),
        FocusedWindow::Right => app.right_panel.handle_events(key),
    }
}
