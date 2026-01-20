use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{App, FocusedWindow};
use crate::players::MusicPlayer;

const POLL_TIMEOUT: Duration = Duration::from_millis(50);

pub fn handle_crossterm_events(app: &mut App) -> color_eyre::Result<()> {
    // Use poll with timeout for responsive playback updates
    if event::poll(POLL_TIMEOUT)? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => on_key_event(app, key),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
    Ok(())
}

pub fn on_key_event(app: &mut App, key: KeyEvent) {
    // Settings popup keybind events first
    if app.settings_panel.opened {
        match key.code {
            KeyCode::Esc => {
                app.settings_panel.close();
                return;
            }
            KeyCode::Tab => {
                app.settings_panel.next_tab();
                return;
            }
            _ => {}
        }
    }

    // Global key events
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
        (_, KeyCode::Char('s')) => {
            app.settings_panel.open();
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
                // Play album starting from selected song
                if let Some(index) = app.center_panel.get_selected_index() {
                    let songs = app.center_panel.get_songs();
                    app.play_album_from(songs, index);
                }
            } else {
                app.center_panel.handle_events(key)
            }
        }
        FocusedWindow::Right => {
            // Playback controls when right panel is focused
            match key.code {
                KeyCode::Char(' ') => app.toggle_pause(),
                KeyCode::Char('n') => app.next_track(),
                KeyCode::Char('p') => app.previous_track(),
                KeyCode::Char('s') => app.stop_playback(),
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    let info = app.player.get_playback_info();
                    let _ = app.player.set_volume(info.volume.saturating_add(5));
                }
                KeyCode::Char('-') => {
                    let info = app.player.get_playback_info();
                    let _ = app.player.set_volume(info.volume.saturating_sub(5));
                }
                KeyCode::Left => {
                    let info = app.player.get_playback_info();
                    let _ = app.player.seek((info.position - 5.0).max(0.0));
                }
                KeyCode::Right => {
                    let info = app.player.get_playback_info();
                    let _ = app.player.seek(info.position + 5.0);
                }
                _ => {}
            }
        }
        FocusedWindow::Logs => app.right_panel.log_panel.handle_events(key),
        FocusedWindow::Settings => app.settings_panel.handle_events(key),
    }
}
