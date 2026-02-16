use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::app::FocusedWindow;

pub enum KeyAction {
    Execute(Action),
    DelegateToPanel,
    None,
}

pub fn resolve_key(key: KeyEvent, focused_window: FocusedWindow, settings_open: bool) -> KeyAction {
    if settings_open {
        return KeyAction::DelegateToPanel;
    }

    // Global keys
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc | KeyCode::Char('q'))
        | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => {
            return KeyAction::Execute(Action::Quit);
        }
        (_, KeyCode::Tab) => return KeyAction::Execute(Action::SwitchPanel),
        (_, KeyCode::Char('s')) if focused_window != FocusedWindow::Right => {
            return KeyAction::Execute(Action::ToggleSettings);
        }
        _ => {}
    }

    // Panel-specific keys
    match focused_window {
        FocusedWindow::Left => match key.code {
            KeyCode::Char(' ') => KeyAction::Execute(Action::SelectAlbum),
            _ => KeyAction::DelegateToPanel,
        },
        FocusedWindow::Center => match key.code {
            KeyCode::Char(' ') => KeyAction::Execute(Action::PlaySelected),
            _ => KeyAction::DelegateToPanel,
        },
        FocusedWindow::Right => match key.code {
            KeyCode::Char(' ') => KeyAction::Execute(Action::TogglePause),
            KeyCode::Char('n') => KeyAction::Execute(Action::NextTrack),
            KeyCode::Char('p') => KeyAction::Execute(Action::PreviousTrack),
            KeyCode::Char('s') => KeyAction::Execute(Action::StopPlayback),
            KeyCode::Char('+') | KeyCode::Char('=') => KeyAction::Execute(Action::VolumeUp(5)),
            KeyCode::Char('-') => KeyAction::Execute(Action::VolumeDown(5)),
            KeyCode::Left => KeyAction::Execute(Action::SeekBackward(5.0)),
            KeyCode::Right => KeyAction::Execute(Action::SeekForward(5.0)),
            _ => KeyAction::None,
        },
        FocusedWindow::Logs => KeyAction::DelegateToPanel,
        FocusedWindow::Settings => KeyAction::DelegateToPanel,
    }
}
