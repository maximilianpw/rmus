use std::time::Duration;

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

use crate::app::App;
use crate::keymap::{resolve_key, KeyAction};

const POLL_TIMEOUT: Duration = Duration::from_millis(50);

pub fn handle_crossterm_events(app: &mut App) -> color_eyre::Result<()> {
    // Use poll with timeout for responsive playback updates
    if event::poll(POLL_TIMEOUT)? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if app.consume_login_notice_key(key) {
                    return Ok(());
                }

                let text_input_active = app.center_panel.is_text_input_active()
                    || app.left_panel.is_filter_input_active();
                match resolve_key(
                    key,
                    app.focused_window,
                    app.settings_panel.opened,
                    app.settings_panel.is_input_active(),
                    text_input_active,
                    app.center_panel.handles_escape() || app.left_panel.handles_escape(),
                ) {
                    KeyAction::Execute(action) => app.execute(action),
                    KeyAction::DelegateToPanel => app.delegate_key_to_panel(key),
                    KeyAction::None => {}
                }
            }
            Event::Mouse(mouse) => {
                if let Some(key) = mouse_scroll_key(mouse.kind) {
                    app.delegate_scroll_at(mouse.column, mouse.row, key);
                } else if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                    app.focus_at(mouse.column, mouse.row);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn mouse_scroll_key(kind: MouseEventKind) -> Option<KeyEvent> {
    match kind {
        MouseEventKind::ScrollUp => Some(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
        MouseEventKind::ScrollDown => Some(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

    use super::mouse_scroll_key;

    #[test]
    fn mouse_scroll_events_map_to_vertical_navigation_keys() {
        assert_eq!(
            mouse_scroll_key(MouseEventKind::ScrollDown).map(|key| key.code),
            Some(KeyCode::Down)
        );
        assert_eq!(
            mouse_scroll_key(MouseEventKind::ScrollUp).map(|key| key.code),
            Some(KeyCode::Up)
        );
    }

    #[test]
    fn non_scroll_mouse_events_are_ignored() {
        assert!(mouse_scroll_key(MouseEventKind::Down(MouseButton::Left)).is_none());
        assert!(mouse_scroll_key(MouseEventKind::Moved).is_none());
    }
}
