use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};

use crate::app::App;
use crate::keymap::{resolve_key, KeyAction};

const POLL_TIMEOUT: Duration = Duration::from_millis(50);

pub fn handle_crossterm_events(app: &mut App) -> color_eyre::Result<()> {
    // Use poll with timeout for responsive playback updates
    if event::poll(POLL_TIMEOUT)? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match resolve_key(key, app.focused_window, app.settings_panel.opened) {
                    KeyAction::Execute(action) => app.execute(action),
                    KeyAction::DelegateToPanel => app.delegate_key_to_panel(key),
                    KeyAction::None => {}
                }
            }
            _ => {}
        }
    }
    Ok(())
}
