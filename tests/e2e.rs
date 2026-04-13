use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, style::Color, Terminal};

use rmus::{
    action::Action,
    app::{App, FocusedWindow},
    config::{AudioConfig, Config, LocalConfig, MaxStreamQuality, TidalConfig},
    sources::{
        song::Song,
        streaming::{
            AuthStatus, ResolvedStream, ResolvedStreamSource, StreamAlbum, StreamTrack,
            StreamingService,
        },
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
        audio: AudioConfig {
            default_volume: 50,
            max_stream_quality: MaxStreamQuality::HiRes,
        },
    }
}

fn make_app(
    qobuz: Option<Box<dyn StreamingService>>,
    tidal: Option<Box<dyn StreamingService>>,
) -> App {
    App::new_for_test(default_config(), qobuz, tidal)
}

fn mock_albums() -> Vec<StreamAlbum> {
    vec![
        StreamAlbum {
            id: "album1".to_string(),
            title: "Album1".to_string(),
            artist: "Artist1".to_string(),
            track_count: Some(2),
        },
        StreamAlbum {
            id: "album2".to_string(),
            title: "Album2".to_string(),
            artist: "Artist2".to_string(),
            track_count: Some(3),
        },
    ]
}

fn mock_album_tracks() -> Vec<StreamTrack> {
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
            artist: "Artist1".to_string(),
            album: "Album1".to_string(),
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
    album_results: Vec<StreamAlbum>,
    album_tracks: Vec<StreamTrack>,
    polls_until_ready: usize,
    poll_count: usize,
    search_delay_ms: u64,
    query_album_results: HashMap<String, Vec<StreamAlbum>>,
    query_delays_ms: HashMap<String, u64>,
}

impl MockStreamingService {
    fn new_authenticated(name: &str, albums: Vec<StreamAlbum>) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: true,
            album_results: albums,
            album_tracks: mock_album_tracks(),
            polls_until_ready: 0,
            poll_count: 0,
            search_delay_ms: 0,
            query_album_results: HashMap::new(),
            query_delays_ms: HashMap::new(),
        }
    }

    fn new_pending(name: &str, polls_needed: usize, albums: Vec<StreamAlbum>) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: false,
            album_results: albums,
            album_tracks: mock_album_tracks(),
            polls_until_ready: polls_needed,
            poll_count: 0,
            search_delay_ms: 0,
            query_album_results: HashMap::new(),
            query_delays_ms: HashMap::new(),
        }
    }

    fn new_authenticated_slow(name: &str, albums: Vec<StreamAlbum>, delay_ms: u64) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: true,
            album_results: albums,
            album_tracks: mock_album_tracks(),
            polls_until_ready: 0,
            poll_count: 0,
            search_delay_ms: delay_ms,
            query_album_results: HashMap::new(),
            query_delays_ms: HashMap::new(),
        }
    }

    fn with_query_behavior(
        mut self,
        query_album_results: HashMap<String, Vec<StreamAlbum>>,
        query_delays_ms: HashMap<String, u64>,
    ) -> Self {
        self.query_album_results = query_album_results;
        self.query_delays_ms = query_delays_ms;
        self
    }

    fn album_results_for_query(query: &str) -> Vec<StreamAlbum> {
        vec![StreamAlbum {
            id: format!("{}-id", query),
            title: format!("{} Album", query),
            artist: "Artist".to_string(),
            track_count: Some(1),
        }]
    }

    fn delay_for_query(&self, query: &str) -> u64 {
        self.query_delays_ms
            .get(query)
            .copied()
            .unwrap_or(self.search_delay_ms)
    }

    fn albums_for_query(&self, query: &str) -> Vec<StreamAlbum> {
        self.query_album_results
            .get(query)
            .cloned()
            .unwrap_or_else(|| self.album_results.clone())
    }
}

impl MockStreamingService {
    fn new_timeout_then_fast(name: &str) -> Self {
        let mut query_album_results = HashMap::new();
        query_album_results.insert("slow".to_string(), Self::album_results_for_query("slow"));
        query_album_results.insert("fast".to_string(), Self::album_results_for_query("fast"));

        let mut query_delays_ms = HashMap::new();
        query_delays_ms.insert("slow".to_string(), 300);
        query_delays_ms.insert("fast".to_string(), 10);

        Self::new_authenticated(name, Vec::new())
            .with_query_behavior(query_album_results, query_delays_ms)
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
        Ok(vec![])
    }

    fn search_albums(
        &mut self,
        query: &str,
        _limit: u32,
    ) -> Result<Vec<StreamAlbum>, Box<dyn std::error::Error>> {
        let delay_ms = self.delay_for_query(query);
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
        Ok(self.albums_for_query(query))
    }

    fn get_album_tracks(
        &mut self,
        _album_id: &str,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        Ok(self.album_tracks.clone())
    }

    fn get_stream_url(
        &mut self,
        _track_id: &str,
    ) -> Result<Option<ResolvedStream>, Box<dyn std::error::Error>> {
        Ok(Some(ResolvedStream {
            source: ResolvedStreamSource::Url("https://example.com/stream.flac".to_string()),
            quality_label: Some("Hi-Res".to_string()),
        }))
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
fn test_left_panel_has_four_tabs() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(200, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Local"), "Should show Local tab");
    assert!(text.contains("Playlists"), "Should show Playlists tab");
    assert!(text.contains("Qobuz"), "Should show Qobuz tab");
    assert!(text.contains("Tidal"), "Should show Tidal tab");
}

#[test]
fn test_search_flow_with_qobuz_tab() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
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

    // Render and verify album results
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Albums (2)"),
        "Should show 'Albums' title with count"
    );
    assert!(text.contains("Artist1 - Album1"), "Should show first album");
    assert!(
        text.contains("Artist2 - Album2"),
        "Should show second album"
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
            ..Default::default()
        },
        Song {
            title: "02 - Disorder.flac".to_string(),
            path: PathBuf::from("/music/album/02.flac"),
            ..Default::default()
        },
        Song {
            title: "03 - She Lost Control.flac".to_string(),
            path: PathBuf::from("/music/album/03.flac"),
            ..Default::default()
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
fn test_album_results_render() {
    let mut app = make_app(None, None);

    // Inject album results directly
    let titles = vec![
        "Artist - Album A (10 tracks)".to_string(),
        "Artist - Album B (8 tracks)".to_string(),
        "Artist - Album C (12 tracks)".to_string(),
    ];
    app.center_panel.set_album_results(titles);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Albums (3)"), "Title should show album count");
    assert!(text.contains("Artist - Album A"));
    assert!(text.contains("Artist - Album B"));
    assert!(text.contains("Artist - Album C"));
}

#[test]
fn test_pending_auth_deferred_search() {
    let mock = MockStreamingService::new_pending("Tidal", 2, mock_albums());
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

    // Render: in AlbumResults mode but with 0 results (search was deferred)
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Albums (0)"),
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
        text.contains("Albums (2)"),
        "Should show 2 albums after auth completes"
    );
    assert!(text.contains("Artist1 - Album1"));
}

#[test]
fn test_search_after_album_select_clears_local_songs() {
    // Regression: selecting a local album then searching on a streaming tab
    // should NOT show the local songs in the search results area.
    let mock = MockStreamingService::new_pending("Tidal", 2, mock_albums());
    let mut app = make_app(None, Some(Box::new(mock)));

    // 1. Set an album on the center panel (simulating Local tab selection)
    let local_songs = vec![
        Song {
            title: "LocalSong1".to_string(),
            ..Default::default()
        },
        Song {
            title: "LocalSong2".to_string(),
            ..Default::default()
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
        "Local songs should NOT appear in album results area"
    );
    assert!(
        text.contains("Albums (0)"),
        "Should show 0 albums while auth is pending"
    );

    // 5. Auth completes after 2 polls, deferred search runs
    app.tick();
    app.tick();

    // Render: Tidal album results should appear
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Artist1 - Album1"),
        "Should show Tidal album results"
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
        audio: AudioConfig {
            default_volume: 50,
            max_stream_quality: MaxStreamQuality::HiRes,
        },
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

#[test]
fn test_search_status_visible_while_background_query_runs() {
    let mock = MockStreamingService::new_authenticated_slow("Qobuz", mock_albums(), 80);
    let mut app = make_app(Some(Box::new(mock)), None);

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "slow".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));

    // Starts async search, but query is still running.
    app.tick();
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Searching Qobuz"),
        "Should show background search status while request is in-flight"
    );

    // Let background worker complete and flush results.
    std::thread::sleep(Duration::from_millis(120));
    app.tick();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Albums (2)"),
        "Should show albums after background search completes"
    );
}

#[test]
fn test_timeout_cancels_stale_search_and_replacement_wins() {
    let mock = MockStreamingService::new_timeout_then_fast("Qobuz");
    let mut app = make_app(Some(Box::new(mock)), None);
    app.set_streaming_timeouts(
        Duration::from_millis(120),
        Duration::from_millis(120),
        Duration::from_millis(120),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "slow".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    // Wait past the test timeout so the slow request is canceled.
    std::thread::sleep(Duration::from_millis(180));
    app.tick();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("timed out"),
        "Should show timeout status after in-flight request exceeds timeout"
    );

    // Trigger a replacement search that should complete quickly.
    app.execute(Action::OpenSearch);
    for c in "fast".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    // Let stale slow result arrive, then process again; it must be ignored.
    std::thread::sleep(Duration::from_millis(220));
    app.tick();

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Artist - fast Album"),
        "Replacement query results should be rendered"
    );
    assert!(
        !text.contains("Artist - slow Album"),
        "Stale canceled query results must be ignored"
    );
}

#[test]
fn test_album_selection_loads_tracks() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let mut app = make_app(Some(Box::new(mock)), None);

    // Search for albums
    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "test".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    // Verify we're in AlbumResults mode
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Albums (2)"));

    // Select first album (press Enter on it)
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    // Verify we're now showing album tracks
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Artist1 - Album1"),
        "Should show album title in header"
    );
    assert!(
        text.contains("2 tracks"),
        "Should show track count in header"
    );
    assert!(text.contains("Track1"), "Should show first track");
    assert!(text.contains("Track2"), "Should show second track");
}

#[test]
fn test_album_tracks_esc_returns_to_albums() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let mut app = make_app(Some(Box::new(mock)), None);

    // Search → albums → select album → tracks
    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "test".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    // Now press Esc to go back to album results
    app.delegate_key_to_panel(make_key(KeyCode::Esc));

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Albums (2)"),
        "Should return to album results after Esc"
    );
    assert!(
        text.contains("Artist1 - Album1"),
        "Album results should still be there"
    );
}
