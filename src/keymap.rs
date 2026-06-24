use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::app::FocusedWindow;

pub enum KeyAction {
    Execute(Action),
    DelegateToPanel,
    None,
}

pub fn resolve_key(
    key: KeyEvent,
    focused_window: FocusedWindow,
    settings_open: bool,
    settings_input_active: bool,
    search_input_active: bool,
    center_panel_handles_escape: bool,
) -> KeyAction {
    if matches!(
        (key.modifiers, key.code),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('c') | KeyCode::Char('C')
        )
    ) {
        return KeyAction::Execute(Action::Quit);
    }

    if settings_open {
        if !settings_input_active && matches!(key.code, KeyCode::Char('s')) {
            return KeyAction::Execute(Action::ToggleSettings);
        }
        if !settings_input_active && matches!(key.code, KeyCode::Char('?')) {
            return KeyAction::Execute(Action::OpenKeybinds);
        }
        return KeyAction::DelegateToPanel;
    }

    if search_input_active {
        return KeyAction::DelegateToPanel;
    }

    // Global keys
    match (key.modifiers, key.code) {
        (_, KeyCode::Esc)
            if matches!(focused_window, FocusedWindow::Center | FocusedWindow::Left)
                && center_panel_handles_escape =>
        {
            return KeyAction::DelegateToPanel;
        }
        (_, KeyCode::Esc | KeyCode::Char('q')) => return KeyAction::Execute(Action::Quit),
        (_, KeyCode::Tab) => return KeyAction::Execute(Action::SwitchPanel),
        (_, KeyCode::Char('s')) if focused_window != FocusedWindow::Right => {
            return KeyAction::Execute(Action::ToggleSettings);
        }
        (_, KeyCode::Char('?')) => return KeyAction::Execute(Action::OpenKeybinds),
        (_, KeyCode::Char('/')) => return KeyAction::Execute(Action::OpenSearch),
        (_, KeyCode::Char('Q')) => return KeyAction::Execute(Action::ShowQueue),
        (_, KeyCode::Char('H')) => return KeyAction::Execute(Action::ShowHistory),
        (_, KeyCode::Char('R')) => return KeyAction::Execute(Action::RefreshLibrary),
        (_, KeyCode::Char('W')) => return KeyAction::Execute(Action::WarmLocalCache),
        (_, KeyCode::Char('x')) => return KeyAction::Execute(Action::TogglePause),
        (_, KeyCode::Char('v')) => return KeyAction::Execute(Action::StopPlayback),
        (_, KeyCode::Char('n')) => return KeyAction::Execute(Action::NextTrack),
        (_, KeyCode::Char('p')) => return KeyAction::Execute(Action::PreviousTrack),
        (_, KeyCode::Char(',')) => return KeyAction::Execute(Action::SeekBackward(5.0)),
        (_, KeyCode::Char('.')) => return KeyAction::Execute(Action::SeekForward(5.0)),
        (_, KeyCode::Char('+') | KeyCode::Char('=')) => {
            return KeyAction::Execute(Action::VolumeUp(5));
        }
        (_, KeyCode::Char('-')) => return KeyAction::Execute(Action::VolumeDown(5)),
        (_, KeyCode::Char('m')) => return KeyAction::Execute(Action::ToggleMute),
        (_, KeyCode::Char('V')) => return KeyAction::Execute(Action::SaveCurrentVolumeAsStartup),
        (_, KeyCode::Char('z')) => return KeyAction::Execute(Action::ToggleShuffle),
        (_, KeyCode::Char('r')) => return KeyAction::Execute(Action::CycleRepeat),
        _ => {}
    }

    // Panel-specific keys
    match focused_window {
        FocusedWindow::Left => match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => KeyAction::Execute(Action::SelectAlbum),
            KeyCode::Char('P') => KeyAction::Execute(Action::PlaySelectedCollection),
            KeyCode::Char('a') => KeyAction::Execute(Action::EnqueueSelectedCollection),
            KeyCode::Char('F') => KeyAction::Execute(Action::AddToFavorites),
            KeyCode::Char('U') => KeyAction::Execute(Action::RemoveFromFavorites),
            KeyCode::Char('f') => KeyAction::Execute(Action::OpenLeftFilter),
            KeyCode::Char('C') => KeyAction::Execute(Action::CreatePlaylist),
            KeyCode::Char('E') => KeyAction::Execute(Action::RenamePlaylist),
            KeyCode::Char('Y') => KeyAction::Execute(Action::DuplicatePlaylist),
            KeyCode::Char('D') => KeyAction::Execute(Action::DeletePlaylist),
            _ => KeyAction::DelegateToPanel,
        },
        FocusedWindow::Center => match key.code {
            KeyCode::Char(' ') => KeyAction::Execute(Action::PlaySelected),
            KeyCode::Char('a') => KeyAction::Execute(Action::EnqueueSelected),
            KeyCode::Char('E') => KeyAction::Execute(Action::EnqueueOpenCollection),
            KeyCode::Char('A') => KeyAction::Execute(Action::AddToPlaylist),
            KeyCode::Char('C') => KeyAction::Execute(Action::AddOpenCollectionToPlaylist),
            KeyCode::Char('F') => KeyAction::Execute(Action::AddToFavorites),
            KeyCode::Char('U') => KeyAction::Execute(Action::RemoveFromFavorites),
            _ => KeyAction::DelegateToPanel,
        },
        FocusedWindow::Right => match key.code {
            KeyCode::Char(' ') => KeyAction::Execute(Action::TogglePause),
            KeyCode::Char('s') => KeyAction::Execute(Action::StopPlayback),
            KeyCode::Char('A') => KeyAction::Execute(Action::AddCurrentTrackToPlaylist),
            KeyCode::Char('F') => KeyAction::Execute(Action::AddToFavorites),
            KeyCode::Char('U') => KeyAction::Execute(Action::RemoveFromFavorites),
            KeyCode::Left => KeyAction::Execute(Action::SeekBackward(5.0)),
            KeyCode::Right => KeyAction::Execute(Action::SeekForward(5.0)),
            _ => KeyAction::None,
        },
        FocusedWindow::Logs => KeyAction::DelegateToPanel,
        FocusedWindow::Settings => KeyAction::DelegateToPanel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn esc_delegates_to_center_when_center_can_close_view() {
        let action = resolve_key(
            key(KeyCode::Esc),
            FocusedWindow::Center,
            false,
            false,
            false,
            true,
        );

        assert!(matches!(action, KeyAction::DelegateToPanel));
    }

    #[test]
    fn esc_delegates_to_left_when_left_can_close_view() {
        let action = resolve_key(
            key(KeyCode::Esc),
            FocusedWindow::Left,
            false,
            false,
            false,
            true,
        );

        assert!(matches!(action, KeyAction::DelegateToPanel));
    }

    #[test]
    fn esc_quits_when_center_has_no_closeable_view() {
        let action = resolve_key(
            key(KeyCode::Esc),
            FocusedWindow::Center,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(action, KeyAction::Execute(Action::Quit)));
    }

    #[test]
    fn q_still_quits_even_when_center_can_close_view() {
        let action = resolve_key(
            key(KeyCode::Char('q')),
            FocusedWindow::Center,
            false,
            false,
            false,
            true,
        );

        assert!(matches!(action, KeyAction::Execute(Action::Quit)));
    }

    #[test]
    fn ctrl_c_quits_even_when_search_input_is_active() {
        let action = resolve_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            FocusedWindow::Center,
            false,
            false,
            true,
            true,
        );

        assert!(matches!(action, KeyAction::Execute(Action::Quit)));
    }

    #[test]
    fn ctrl_c_quits_even_when_settings_are_open() {
        let action = resolve_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            FocusedWindow::Center,
            true,
            false,
            false,
            true,
        );

        assert!(matches!(action, KeyAction::Execute(Action::Quit)));
    }

    #[test]
    fn s_toggles_settings_when_settings_are_open() {
        let action = resolve_key(
            key(KeyCode::Char('s')),
            FocusedWindow::Center,
            true,
            false,
            false,
            false,
        );

        assert!(matches!(action, KeyAction::Execute(Action::ToggleSettings)));
    }

    #[test]
    fn s_delegates_when_settings_input_is_active() {
        let action = resolve_key(
            key(KeyCode::Char('s')),
            FocusedWindow::Center,
            true,
            true,
            false,
            false,
        );

        assert!(matches!(action, KeyAction::DelegateToPanel));
    }

    #[test]
    fn q_shows_queue_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
        ] {
            let action = resolve_key(key(KeyCode::Char('Q')), focus, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::ShowQueue)));
        }
    }

    #[test]
    fn question_mark_opens_keybinds_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
            FocusedWindow::Logs,
        ] {
            let action = resolve_key(key(KeyCode::Char('?')), focus, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::OpenKeybinds)));
        }
    }

    #[test]
    fn question_mark_opens_keybinds_when_settings_are_open_without_text_input() {
        let action = resolve_key(
            key(KeyCode::Char('?')),
            FocusedWindow::Settings,
            true,
            false,
            false,
            false,
        );

        assert!(matches!(action, KeyAction::Execute(Action::OpenKeybinds)));
    }

    #[test]
    fn left_panel_space_and_enter_open_selected_item() {
        for code in [KeyCode::Char(' '), KeyCode::Enter] {
            let action = resolve_key(key(code), FocusedWindow::Left, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::SelectAlbum)));
        }
    }

    #[test]
    fn left_panel_uppercase_e_renames_selected_playlist() {
        let action = resolve_key(
            key(KeyCode::Char('E')),
            FocusedWindow::Left,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(action, KeyAction::Execute(Action::RenamePlaylist)));
    }

    #[test]
    fn left_panel_uppercase_y_duplicates_selected_playlist() {
        let action = resolve_key(
            key(KeyCode::Char('Y')),
            FocusedWindow::Left,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(
            action,
            KeyAction::Execute(Action::DuplicatePlaylist)
        ));
    }

    #[test]
    fn left_panel_collection_shortcuts_play_and_queue_selected_item() {
        let play = resolve_key(
            key(KeyCode::Char('P')),
            FocusedWindow::Left,
            false,
            false,
            false,
            false,
        );
        assert!(matches!(
            play,
            KeyAction::Execute(Action::PlaySelectedCollection)
        ));

        let enqueue = resolve_key(
            key(KeyCode::Char('a')),
            FocusedWindow::Left,
            false,
            false,
            false,
            false,
        );
        assert!(matches!(
            enqueue,
            KeyAction::Execute(Action::EnqueueSelectedCollection)
        ));
    }

    #[test]
    fn left_panel_f_opens_list_filter() {
        let action = resolve_key(
            key(KeyCode::Char('f')),
            FocusedWindow::Left,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(action, KeyAction::Execute(Action::OpenLeftFilter)));
    }

    #[test]
    fn center_panel_uppercase_e_queues_open_collection() {
        let action = resolve_key(
            key(KeyCode::Char('E')),
            FocusedWindow::Center,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(
            action,
            KeyAction::Execute(Action::EnqueueOpenCollection)
        ));
    }

    #[test]
    fn center_panel_uppercase_c_adds_open_collection_to_playlist() {
        let action = resolve_key(
            key(KeyCode::Char('C')),
            FocusedWindow::Center,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(
            action,
            KeyAction::Execute(Action::AddOpenCollectionToPlaylist)
        ));
    }

    #[test]
    fn text_input_delegates_global_characters_to_panel() {
        let action = resolve_key(
            key(KeyCode::Char('s')),
            FocusedWindow::Center,
            false,
            false,
            true,
            true,
        );

        assert!(matches!(action, KeyAction::DelegateToPanel));
    }

    #[test]
    fn uppercase_r_refreshes_library_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
            FocusedWindow::Logs,
        ] {
            let action = resolve_key(key(KeyCode::Char('R')), focus, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::RefreshLibrary)));
        }
    }

    #[test]
    fn uppercase_w_warms_local_cache_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
            FocusedWindow::Logs,
        ] {
            let action = resolve_key(key(KeyCode::Char('W')), focus, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::WarmLocalCache)));
        }
    }

    #[test]
    fn uppercase_h_shows_history_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
            FocusedWindow::Logs,
        ] {
            let action = resolve_key(key(KeyCode::Char('H')), focus, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::ShowHistory)));
        }
    }

    #[test]
    fn right_panel_uppercase_a_adds_current_track_to_playlist() {
        let action = resolve_key(
            key(KeyCode::Char('A')),
            FocusedWindow::Right,
            false,
            false,
            false,
            false,
        );

        assert!(matches!(
            action,
            KeyAction::Execute(Action::AddCurrentTrackToPlaylist)
        ));
    }

    #[test]
    fn uppercase_f_adds_selected_or_current_track_to_favorites() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
        ] {
            let action = resolve_key(key(KeyCode::Char('F')), focus, false, false, false, false);

            assert!(matches!(action, KeyAction::Execute(Action::AddToFavorites)));
        }
    }

    #[test]
    fn uppercase_u_removes_selected_or_current_track_from_favorites() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
        ] {
            let action = resolve_key(key(KeyCode::Char('U')), focus, false, false, false, false);

            assert!(matches!(
                action,
                KeyAction::Execute(Action::RemoveFromFavorites)
            ));
        }
    }

    #[test]
    fn playback_controls_work_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
            FocusedWindow::Logs,
        ] {
            let next = resolve_key(key(KeyCode::Char('n')), focus, false, false, false, false);
            assert!(matches!(next, KeyAction::Execute(Action::NextTrack)));

            let previous = resolve_key(key(KeyCode::Char('p')), focus, false, false, false, false);
            assert!(matches!(
                previous,
                KeyAction::Execute(Action::PreviousTrack)
            ));

            let pause = resolve_key(key(KeyCode::Char('x')), focus, false, false, false, false);
            assert!(matches!(pause, KeyAction::Execute(Action::TogglePause)));

            let stop = resolve_key(key(KeyCode::Char('v')), focus, false, false, false, false);
            assert!(matches!(stop, KeyAction::Execute(Action::StopPlayback)));

            let seek_backward =
                resolve_key(key(KeyCode::Char(',')), focus, false, false, false, false);
            assert!(matches!(
                seek_backward,
                KeyAction::Execute(Action::SeekBackward(5.0))
            ));

            let seek_forward =
                resolve_key(key(KeyCode::Char('.')), focus, false, false, false, false);
            assert!(matches!(
                seek_forward,
                KeyAction::Execute(Action::SeekForward(5.0))
            ));

            let volume_up = resolve_key(key(KeyCode::Char('+')), focus, false, false, false, false);
            assert!(matches!(volume_up, KeyAction::Execute(Action::VolumeUp(5))));

            let volume_down =
                resolve_key(key(KeyCode::Char('-')), focus, false, false, false, false);
            assert!(matches!(
                volume_down,
                KeyAction::Execute(Action::VolumeDown(5))
            ));

            let mute = resolve_key(key(KeyCode::Char('m')), focus, false, false, false, false);
            assert!(matches!(mute, KeyAction::Execute(Action::ToggleMute)));

            let shuffle = resolve_key(key(KeyCode::Char('z')), focus, false, false, false, false);
            assert!(matches!(shuffle, KeyAction::Execute(Action::ToggleShuffle)));

            let repeat = resolve_key(key(KeyCode::Char('r')), focus, false, false, false, false);
            assert!(matches!(repeat, KeyAction::Execute(Action::CycleRepeat)));
        }
    }

    #[test]
    fn uppercase_v_saves_current_volume_from_main_panels() {
        for focus in [
            FocusedWindow::Left,
            FocusedWindow::Center,
            FocusedWindow::Right,
            FocusedWindow::Logs,
        ] {
            let action = resolve_key(key(KeyCode::Char('V')), focus, false, false, false, false);

            assert!(matches!(
                action,
                KeyAction::Execute(Action::SaveCurrentVolumeAsStartup)
            ));
        }
    }
}
