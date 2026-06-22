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
            _ => {}
        }
    }
    Ok(())
}
