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
        _ => {}
    }

    // Focused window specific key events
    match app.focused_window {
        FocusedWindow::Left => {
            if key.code == KeyCode::Char(' ') {
                if let Some((path, songs)) = app.left_panel.get_selected_album() {
                    app.center_panel.set_album(path, songs);
                }
            } else {
                app.left_panel.handle_events(key);
            }
        }
        FocusedWindow::Center => {
            if key.code == KeyCode::Char(' ') {
                if let Some(song) = app.center_panel.get_selected_song() {
                    app.right_panel.play_song(song)
                }
            } else {
                app.center_panel.handle_events(key)
            }
        }
        FocusedWindow::Right => {}
        FocusedWindow::Logs => app.right_panel.log_panel.handle_events(key),
    }
}
