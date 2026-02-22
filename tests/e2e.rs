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
    }
}

fn make_app(
    qobuz: Option<Box<dyn StreamingService>>,
    tidal: Option<Box<dyn StreamingService>>,
) -> App {
    App::new_for_test(default_config(), qobuz, tidal)
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
    service_name: String,
    authenticated: bool,
    search_results: Vec<StreamTrack>,
    polls_until_ready: usize,
    poll_count: usize,
}

impl MockStreamingService {
    fn new_authenticated(name: &str, results: Vec<StreamTrack>) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: true,
            search_results: results,
            polls_until_ready: 0,
            poll_count: 0,
        }
    }

    fn new_pending(name: &str, polls_needed: usize, results: Vec<StreamTrack>) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: false,
            search_results: results,
            polls_until_ready: polls_needed,
            poll_count: 0,
        }
    }
}

impl StreamingService for MockStreamingService {
    fn name(&self) -> &str {
        &self.service_name
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

/// Helper: navigate the left panel to a specific tab by name.
fn switch_to_tab(app: &mut App, target: &str) {
    // Press Right on the left panel until we land on the target tab
    for _ in 0..5 {
        if app.left_panel.active_tab_name() == target {
            return;
        }
        app.left_panel.handle_events(make_key(KeyCode::Right));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_app_renders_without_panic() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
}

#[test]
fn test_left_panel_has_three_tabs() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Local"), "Should show Local tab");
    assert!(text.contains("Qobuz"), "Should show Qobuz tab");
    assert!(text.contains("Tidal"), "Should show Tidal tab");
}

#[test]
fn test_search_flow_with_qobuz_tab() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_tracks());
    let mut app = make_app(Some(Box::new(mock)), None);

    // Switch to Qobuz tab so search targets it
    switch_to_tab(&mut app, "Qobuz");

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
    assert!(text.contains("Artist1 - Track1"), "Should show first track");
    assert!(
        text.contains("Artist2 - Track2"),
        "Should show second track"
    );
}

#[test]
fn test_local_search_filters_album_songs() {
    let mut app = make_app(None, None);

    // Load songs into center panel (simulating album selection)
    let songs = vec![
        Song {
            title: "01 - Love Will Tear Us Apart.flac".to_string(),
            path: PathBuf::from("/music/album/01.flac"),
            url: None,
        },
        Song {
            title: "02 - Disorder.flac".to_string(),
            path: PathBuf::from("/music/album/02.flac"),
            url: None,
        },
        Song {
            title: "03 - She Lost Control.flac".to_string(),
            path: PathBuf::from("/music/album/03.flac"),
            url: None,
        },
    ];
    app.center_panel
        .set_album(PathBuf::from("/music/album"), songs);

    // On Local tab, open search (now allowed since songs are loaded)
    assert_eq!(app.left_panel.active_tab_name(), "Local");
    app.execute(Action::OpenSearch);
    assert_eq!(
        app.focused_window,
        FocusedWindow::Center,
        "Search should open on Local tab with songs"
    );

    // Type "love" and submit
    for c in "love".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    // Render: should show filtered results
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Love Will Tear Us Apart"),
        "Matching song should appear"
    );
    assert!(
        !text.contains("Disorder"),
        "Non-matching song should be filtered out"
    );
    assert!(
        text.contains("Search Results (1)"),
        "Should show 1 filtered result"
    );

    // Esc should restore all album songs
    app.delegate_key_to_panel(make_key(KeyCode::Esc));
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Disorder"),
        "All songs should be restored after Esc"
    );
    assert!(
        text.contains("Love Will Tear Us Apart"),
        "All songs should be restored after Esc"
    );
}

#[test]
fn test_search_on_local_tab_shows_hint() {
    let mut app = make_app(None, None);

    // Stay on Local tab (default)
    assert_eq!(app.left_panel.active_tab_name(), "Local");

    // Try to open search — should not switch to center panel
    app.execute(Action::OpenSearch);
    assert_eq!(
        app.focused_window,
        FocusedWindow::Left,
        "Search should not open on Local tab"
    );
}

#[test]
fn test_search_results_render() {
    let mut app = make_app(None, None);

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
    let mock = MockStreamingService::new_pending("Tidal", 2, mock_tracks());
    let mut app = make_app(None, Some(Box::new(mock)));

    // Switch to Tidal tab
    switch_to_tab(&mut app, "Tidal");

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
fn test_search_after_album_select_clears_local_songs() {
    // Regression: selecting a local album then searching on a streaming tab
    // should NOT show the local songs in the search results area.
    let mock = MockStreamingService::new_pending("Tidal", 2, mock_tracks());
    let mut app = make_app(None, Some(Box::new(mock)));

    // 1. Set an album on the center panel (simulating Local tab selection)
    let local_songs = vec![
        Song {
            title: "LocalSong1".to_string(),
            path: PathBuf::new(),
            url: None,
        },
        Song {
            title: "LocalSong2".to_string(),
            path: PathBuf::new(),
            url: None,
        },
    ];
    app.center_panel
        .set_album(PathBuf::from("/music/album"), local_songs);

    // 2. Switch to Tidal tab
    switch_to_tab(&mut app, "Tidal");

    // 3. Open search, type query, submit
    app.execute(Action::OpenSearch);
    for c in "hello".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));

    // 4. First tick — auth is pending, search is deferred
    app.tick();

    // Render: should show empty results, NOT local songs
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("LocalSong1"),
        "Local songs should NOT appear in search results area"
    );
    assert!(
        text.contains("Search Results (0)"),
        "Should show 0 results while auth is pending"
    );

    // 5. Auth completes after 2 polls, deferred search runs
    app.tick();
    app.tick();

    // Render: Tidal search results should appear
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Artist1 - Track1"),
        "Should show Tidal search results"
    );
    assert!(
        !text.contains("LocalSong1"),
        "Local songs should still not appear"
    );
}

#[test]
fn test_panel_switching() {
    let mut app = make_app(None, None);
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
    let mut app = make_app(None, None);
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
fn test_account_settings_shows_both_services() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Open settings
    app.execute(Action::ToggleSettings);

    // Navigate to Account tab (Tab from General → Account)
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    // Render: should show both Qobuz and Tidal sections
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Qobuz"), "Should show Qobuz section");
    assert!(text.contains("Tidal"), "Should show Tidal section");
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
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(80, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    // Open settings
    app.execute(Action::ToggleSettings);

    // Navigate to Account tab
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    // Render: Tidal section shows "Authenticated" since token is present
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Authenticated"),
        "Tidal with token should show 'Authenticated'"
    );
}

#[test]
fn test_keybind_tab_shows_generic_search_text() {
    let mut app = make_app(None, None);
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
