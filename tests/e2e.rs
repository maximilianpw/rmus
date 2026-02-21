use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};

use rmus::{
    action::Action,
    app::{App, FocusedWindow},
    config::{AudioConfig, Config, LocalConfig, TidalConfig},
    sources::{
        song::Song,
        streaming::{AuthStatus, StreamTrack, StreamingService},
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn extract_buffer_text(buffer: &Buffer) -> String {
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                text.push_str(cell.symbol());
            }
        }
        text.push('\n');
    }
    text
}

fn default_config() -> Config {
    Config {
        local: LocalConfig {
            sources: Vec::new(),
        },
        qobuz: None,
        tidal: None,
        audio: AudioConfig { default_volume: 50 },
        streaming_service: "qobuz".to_string(),
    }
}

fn make_app(streaming: Option<Box<dyn StreamingService>>) -> App {
    App::new_for_test(default_config(), streaming)
}

fn mock_tracks() -> Vec<StreamTrack> {
    vec![
        StreamTrack {
            id: "1".to_string(),
            title: "Track1".to_string(),
            artist: "Artist1".to_string(),
            album: "Album1".to_string(),
        },
        StreamTrack {
            id: "2".to_string(),
            title: "Track2".to_string(),
            artist: "Artist2".to_string(),
            album: "Album2".to_string(),
        },
    ]
}

// ---------------------------------------------------------------------------
// MockStreamingService
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MockStreamingService {
    authenticated: bool,
    search_results: Vec<StreamTrack>,
    polls_until_ready: usize,
    poll_count: usize,
}

impl MockStreamingService {
    fn new_authenticated(results: Vec<StreamTrack>) -> Self {
        Self {
            authenticated: true,
            search_results: results,
            polls_until_ready: 0,
            poll_count: 0,
        }
    }

    fn new_pending(polls_needed: usize, results: Vec<StreamTrack>) -> Self {
        Self {
            authenticated: false,
            search_results: results,
            polls_until_ready: polls_needed,
            poll_count: 0,
        }
    }
}

impl StreamingService for MockStreamingService {
    fn name(&self) -> &str {
        "Mock"
    }

    fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    fn authenticate(&mut self) -> Result<AuthStatus, Box<dyn std::error::Error>> {
        if self.authenticated {
            Ok(AuthStatus::Authenticated)
        } else {
            Ok(AuthStatus::PendingUserAction(
                "Please authorize at: https://example.com".to_string(),
            ))
        }
    }

    fn poll_auth(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        self.poll_count += 1;
        if self.poll_count >= self.polls_until_ready {
            self.authenticated = true;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn search(
        &mut self,
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        Ok(self.search_results.clone())
    }

    fn get_stream_url(
        &mut self,
        _track_id: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
        Ok(Some("https://example.com/stream.flac".to_string()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_app_renders_without_panic() {
    let mut app = make_app(None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_search_flow_with_mock_service() {
    let mock = MockStreamingService::new_authenticated(mock_tracks());
    let mut app = make_app(Some(Box::new(mock)));

    // Open search
    app.execute(Action::OpenSearch);

    // Type query
    for c in "test".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }

    // Submit search
    app.delegate_key_to_panel(make_key(KeyCode::Enter));

    // Process the pending query
    app.tick();

    // Render and verify results
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Search Results"),
        "Should show 'Search Results' title"
    );
    assert!(
        text.contains("Artist1 - Track1"),
        "Should show first track"
    );
    assert!(
        text.contains("Artist2 - Track2"),
        "Should show second track"
    );
}

#[test]
fn test_search_results_render() {
    let mut app = make_app(None);

    // Inject search results directly
    let songs = vec![
        Song {
            title: "Artist - Song A".to_string(),
            path: PathBuf::new(),
            url: None,
        },
        Song {
            title: "Artist - Song B".to_string(),
            path: PathBuf::new(),
            url: None,
        },
        Song {
            title: "Artist - Song C".to_string(),
            path: PathBuf::new(),
            url: None,
        },
    ];
    app.center_panel.set_search_results(songs);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Search Results (3)"),
        "Title should show result count"
    );
    assert!(text.contains("Artist - Song A"));
    assert!(text.contains("Artist - Song B"));
    assert!(text.contains("Artist - Song C"));
}

#[test]
fn test_pending_auth_deferred_search() {
    let mock = MockStreamingService::new_pending(2, mock_tracks());
    let mut app = make_app(Some(Box::new(mock)));

    // Open search, type query, submit
    app.execute(Action::OpenSearch);
    for c in "hello".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));

    // First tick: perform_search → auth pending → search deferred
    app.tick();

    // Render: in SearchResults mode but with 0 results (search was deferred)
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Search Results (0)"),
        "No results yet while auth is pending"
    );

    // Second tick: poll_auth returns false (1st poll)
    app.tick();

    // Third tick: poll_auth returns true (2nd poll) → deferred search executes
    app.tick();

    // Render: results should now appear
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Search Results (2)"),
        "Should show 2 results after auth completes"
    );
    assert!(text.contains("Artist1 - Track1"));
}

#[test]
fn test_panel_switching() {
    let mut app = make_app(None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Initial: Left panel focused
    assert_eq!(app.focused_window, FocusedWindow::Left);
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    // Left panel top-left border corner should be Yellow (focused)
    assert_eq!(
        frame.buffer.cell((0, 0)).unwrap().fg,
        Color::Yellow,
        "Focused left panel border should be Yellow"
    );

    // Tab → Center
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Center);
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    // Left panel border no longer Yellow
    assert_ne!(
        frame.buffer.cell((0, 0)).unwrap().fg,
        Color::Yellow,
        "Unfocused left panel border should not be Yellow"
    );

    // Tab → Right
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Right);

    // Tab → Logs
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Logs);

    // Tab → wraps back to Left
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Left);
}

#[test]
fn test_settings_opens_and_closes() {
    let mut app = make_app(None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Settings initially closed
    assert!(!app.settings_panel.opened);

    // Open settings
    app.execute(Action::ToggleSettings);
    assert!(app.settings_panel.opened);

    // Render and verify settings overlay is visible
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Settings"),
        "Settings overlay should be visible"
    );
    assert!(text.contains("General"), "General tab should be visible");

    // Close with Esc
    app.delegate_key_to_panel(make_key(KeyCode::Esc));
    assert!(!app.settings_panel.opened);

    // Render and verify settings overlay is gone
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains(" Settings "),
        "Settings overlay should not be visible after closing"
    );
}

#[test]
fn test_account_settings_service_selector() {
    let mut app = make_app(None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Open settings
    app.execute(Action::ToggleSettings);

    // Navigate to Account tab (Tab from General → Account)
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    // Render: should show "Qobuz" as default service
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Qobuz"),
        "Default service should be Qobuz"
    );

    // Press Right → switch to Tidal
    app.delegate_key_to_panel(make_key(KeyCode::Right));
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Tidal"), "Should show Tidal after Right");

    // Press Left → back to Qobuz
    app.delegate_key_to_panel(make_key(KeyCode::Left));
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Qobuz"),
        "Should show Qobuz after Left"
    );
}

#[test]
fn test_account_settings_tidal_status() {
    let config = Config {
        local: LocalConfig {
            sources: Vec::new(),
        },
        qobuz: None,
        tidal: Some(TidalConfig {
            access_token: "abc123".to_string(),
            refresh_token: "def456".to_string(),
            country_code: "US".to_string(),
            token_expiry: 1700000000,
        }),
        audio: AudioConfig { default_volume: 50 },
        streaming_service: "tidal".to_string(),
    };
    let mut app = App::new_for_test(config, None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Open settings
    app.execute(Action::ToggleSettings);

    // Navigate to Account tab
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    // Render: Tidal is pre-selected (streaming_service = "tidal"), token present
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Authenticated"),
        "Tidal with token should show 'Authenticated'"
    );
}

#[test]
fn test_keybind_tab_shows_generic_search_text() {
    let mut app = make_app(None);
    // Use a taller terminal so all keybinds are visible in the settings popup
    let backend = TestBackend::new(100, 50);
    let mut terminal = Terminal::new(backend).unwrap();

    // Open settings
    app.execute(Action::ToggleSettings);

    // Navigate to Keybinds tab (Tab twice: General → Account → Keybinds)
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    // Render and verify keybinds content
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    // "/" should show generic "Open search", not service-specific text
    assert!(
        text.contains("Open search"),
        "Search keybind should show generic 'Open search' text"
    );
    assert!(
        !text.contains("Open search (Qobuz)"),
        "Should NOT show service-specific search text"
    );
    assert!(
        !text.contains("Open search (Tidal)"),
        "Should NOT show service-specific search text"
    );
}
