use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

use rmus::{
    action::Action,
    app::{App, FocusedWindow},
    config::{
        AudioConfig, Config, LocalConfig, LocalSource, MaxStreamQuality, QobuzConfig, TidalConfig,
    },
    history::HistoryStore,
    keymap::{resolve_key, KeyAction},
    players::{MusicPlayer, PlaybackInfo, PlaybackState, PlayerResult, RepeatMode, ShuffleMode},
    playlist::{Playlist, PlaylistStore},
    queue::{QueueState, QueueStore},
    sources::{
        song::Song,
        streaming::{
            AuthStatus, ResolvedStream, ResolvedStreamSource, StreamAlbum, StreamTrack,
            StreamingService,
        },
    },
    ui::theme,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn dispatch_key(app: &mut App, key: KeyEvent) {
    let text_input_active =
        app.center_panel.is_text_input_active() || app.left_panel.is_filter_input_active();
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

fn render_app_text(app: &mut App, terminal: &mut Terminal<TestBackend>) -> String {
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    extract_buffer_text(frame.buffer)
}

fn tick_until_rendered(
    app: &mut App,
    terminal: &mut Terminal<TestBackend>,
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> String {
    let deadline = Instant::now() + timeout;
    let mut text = render_app_text(app, terminal);

    loop {
        if predicate(&text) || Instant::now() >= deadline {
            return text;
        }

        std::thread::sleep(Duration::from_millis(10));
        app.tick();
        text = render_app_text(app, terminal);
    }
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
            default_shuffle: ShuffleMode::Off,
            default_repeat: RepeatMode::Off,
        },
    }
}

fn test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rmus-e2e-{name}-{nanos}"))
}

fn make_app(
    qobuz: Option<Box<dyn StreamingService>>,
    tidal: Option<Box<dyn StreamingService>>,
) -> App {
    App::new_for_test(default_config(), qobuz, tidal)
}

fn rmus_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rmus"))
}

fn fake_mpv_bin_dir(name: &str) -> PathBuf {
    let dir = test_dir(name).join("bin");
    std::fs::create_dir_all(&dir).unwrap();
    let executable_name = if cfg!(windows) { "mpv.exe" } else { "mpv" };
    let path = dir.join(executable_name);
    std::fs::write(&path, "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

fn find_file_named(root: &Path, file_name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(root).ok()?.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file_named(&path, file_name) {
                return Some(found);
            }
        }
    }
    None
}

fn state_env(command: &mut Command, state_dir: &Path) {
    let config_dir = state_dir.join("rmus-config");
    let cache_dir = state_dir.join("rmus-cache");
    command
        .env("HOME", state_dir)
        .env("USERPROFILE", state_dir)
        .env("XDG_CONFIG_HOME", state_dir.join("xdg-config"))
        .env("APPDATA", state_dir.join("appdata"))
        .env("LOCALAPPDATA", state_dir.join("localappdata"))
        .env("RMUS_CONFIG_DIR", config_dir)
        .env("RMUS_CACHE_DIR", cache_dir)
        .current_dir(state_dir);
}

fn isolated_storage_path(state_dir: &Path, label: &str) -> PathBuf {
    let mut command = rmus_binary();
    state_env(command.arg("paths"), state_dir);
    let output = command.output().expect("rmus paths should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let prefix = format!("{label}: ");
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("rmus paths should print {label} path"))
}

fn isolated_config_path(state_dir: &Path) -> PathBuf {
    isolated_storage_path(state_dir, "config")
}

fn isolated_playlists_dir(state_dir: &Path) -> PathBuf {
    isolated_storage_path(state_dir, "playlists")
}

#[test]
fn test_cli_version_prints_without_launching_tui() {
    let output = rmus_binary()
        .arg("--version")
        .output()
        .expect("rmus --version should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("rmus {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn test_cli_help_prints_without_launching_tui() {
    let output = rmus_binary()
        .arg("--help")
        .output()
        .expect("rmus --help should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("keyboard-driven terminal music player"));
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("paths"));
    assert!(stdout.contains("completions"));
    assert!(stdout.contains("list-sources"));
    assert!(stdout.contains("list-playlists"));
    assert!(stdout.contains("show-history"));
    assert!(stdout.contains("show-queue"));
    assert!(stdout.contains("local-stats"));
    assert!(stdout.contains("search-local"));
    assert!(stdout.contains("scan-local"));
    assert!(stdout.contains("add-source"));
    assert!(stdout.contains("remove-source"));
    assert!(stdout.contains("move-source"));
    assert!(stdout.contains("show-playlist"));
    assert!(stdout.contains("delete-playlist"));
    assert!(stdout.contains("import-playlist"));
    assert!(stdout.contains("export-playlist"));
    assert!(stdout.contains("clear-cache"));
    assert!(stdout.contains("clear-history"));
    assert!(stdout.contains("clear-queue"));
    assert!(stdout.contains("--version"));
}

#[test]
fn test_cli_completions_print_without_launching_tui() {
    let output = rmus_binary()
        .args(["completions", "fish"])
        .output()
        .expect("rmus completions fish should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("complete -c rmus"));
    assert!(stdout.contains("show-playlist"));
    assert!(stdout.contains("-l limit"));

    let missing = rmus_binary()
        .arg("completions")
        .output()
        .expect("rmus completions without shell should run");
    assert!(!missing.status.success());
    let stderr = String::from_utf8(missing.stderr).unwrap();
    assert!(stderr.contains("missing shell for completions"));
}

#[test]
fn test_cli_status_prints_saved_state_without_launching_tui() {
    let state_dir = test_dir("cli-status");
    std::fs::create_dir_all(&state_dir).unwrap();
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();

    let config_path = isolated_storage_path(&state_dir, "config");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![
                LocalSource {
                    name: "Library".to_string(),
                    path: music_dir.clone(),
                },
                LocalSource {
                    name: "Missing".to_string(),
                    path: state_dir.join("missing"),
                },
            ],
        },
        qobuz: Some(QobuzConfig {
            email: "listener@example.com".to_string(),
            password: "secret".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        tidal: Some(TidalConfig {
            access_token: "tidal-access".to_string(),
            refresh_token: "tidal-refresh".to_string(),
            country_code: "US".to_string(),
            token_expiry: 4_102_444_800,
        }),
        audio: AudioConfig {
            default_volume: 64,
            max_stream_quality: MaxStreamQuality::Cd,
            default_shuffle: ShuffleMode::On,
            default_repeat: RepeatMode::All,
        },
    };
    std::fs::write(config_path, toml::to_string_pretty(&config).unwrap()).unwrap();

    let playlists_dir = isolated_storage_path(&state_dir, "playlists");
    let playlist_store = PlaylistStore::with_dir(playlists_dir);
    playlist_store.create("Road".to_string()).unwrap();
    HistoryStore::with_path(isolated_storage_path(&state_dir, "history"))
        .save(&[Song {
            title: "History Song".to_string(),
            artist: "History Artist".to_string(),
            path: music_dir.join("history.flac"),
            ..Default::default()
        }])
        .unwrap();
    QueueStore::with_path(isolated_storage_path(&state_dir, "queue"))
        .save(&QueueState::new(
            vec![
                Song {
                    title: "First Queue".to_string(),
                    artist: "Queue Artist".to_string(),
                    path: music_dir.join("first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second Queue".to_string(),
                    artist: "Queue Artist".to_string(),
                    album_name: "Queue Album".to_string(),
                    path: music_dir.join("second.flac"),
                    ..Default::default()
                },
            ],
            1,
        ))
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("status"), &state_dir);
    let output = command.output().expect("rmus status should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rmus status"));
    assert!(stdout.contains("Local sources: 2 configured sources, 1 missing"));
    assert!(stdout.contains("Qobuz: configured"));
    assert!(stdout.contains("Tidal: authenticated"));
    assert!(stdout.contains("Audio: volume 64%, quality CD, shuffle On, repeat All"));
    assert!(stdout.contains("Playlists: 1 playlist"));
    assert!(stdout.contains("History: 1 track"));
    assert!(stdout.contains("Saved queue: 2 tracks, position 2 of 2"));
    assert!(
        stdout.contains("Current saved track: Queue Artist - Second Queue (Queue Album) [local]")
    );
    assert!(stdout.contains("second.flac"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_paths_prints_storage_paths_without_launching_tui() {
    let state_dir = test_dir("cli-paths");
    std::fs::create_dir_all(&state_dir).unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("paths"), &state_dir);
    let output = command.output().expect("rmus paths should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rmus paths"));
    assert!(stdout.contains("config:"));
    assert!(stdout.contains("playlists:"));
    assert!(stdout.contains("history:"));
    assert!(stdout.contains("queue:"));
    assert!(stdout.contains("local cache:"));
    assert!(stdout.contains("local-cache.toml"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_search_local_reports_matching_tracks_without_launching_tui() {
    let state_dir = test_dir("cli-search-local");
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("01 - First.flac"), "not real audio").unwrap();
    std::fs::write(music_dir.join("02 - Second.opus"), "not real audio").unwrap();

    let mut add_command = rmus_binary();
    state_env(
        add_command.args([
            "add-source",
            "Library",
            music_dir.to_str().unwrap(),
            "--scan",
        ]),
        &state_dir,
    );
    let add_output = add_command
        .output()
        .expect("rmus add-source --scan should run");
    assert!(add_output.status.success());

    let mut search_command = rmus_binary();
    state_env(
        search_command.args(["search-local", "second", "--limit", "1"]),
        &state_dir,
    );
    let output = search_command
        .output()
        .expect("rmus search-local should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Local search 'second': 1 match, showing 1"));
    assert!(stdout.contains("1 configured source"));
    assert!(stdout.contains("02 - Second.opus [Library]"));
    assert!(stdout.contains("02 - Second.opus"));

    let mut empty_command = rmus_binary();
    state_env(empty_command.args(["search-local", "missing"]), &state_dir);
    let empty_output = empty_command
        .output()
        .expect("empty rmus search-local should run");
    assert!(empty_output.status.success());
    let empty_stdout = String::from_utf8(empty_output.stdout).unwrap();
    assert!(empty_stdout.contains("Local search 'missing': 0 matches"));
    assert!(empty_stdout.contains("No matching local tracks."));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_add_source_updates_config_without_launching_tui() {
    let state_dir = test_dir("cli-add-source");
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("01 - First.flac"), "not real audio").unwrap();

    let mut add_command = rmus_binary();
    state_env(
        add_command.args([
            "add-source",
            "Library",
            music_dir.join(".").to_str().unwrap(),
        ]),
        &state_dir,
    );
    let add_output = add_command.output().expect("rmus add-source should run");

    assert!(add_output.status.success());
    assert!(add_output.stderr.is_empty());
    let add_stdout = String::from_utf8(add_output.stdout).unwrap();
    assert!(add_stdout.contains("Added local source 'Library'"));
    assert!(add_stdout.contains("1 configured source"));

    let config_path = isolated_config_path(&state_dir);
    let config = std::fs::read_to_string(&config_path).unwrap();
    assert!(config.contains("name = \"Library\""));
    assert!(config.contains(
        &music_dir
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string()
    ));

    let mut stats_command = rmus_binary();
    state_env(stats_command.arg("local-stats"), &state_dir);
    let stats_output = stats_command.output().expect("rmus local-stats should run");
    assert!(stats_output.status.success());
    let stats_stdout = String::from_utf8(stats_output.stdout).unwrap();
    assert!(stats_stdout.contains("Local library: 1 configured source"));
    assert!(stats_stdout.contains("1 playable track"));

    let mut duplicate_command = rmus_binary();
    state_env(
        duplicate_command.args(["add-source", "Other", music_dir.to_str().unwrap()]),
        &state_dir,
    );
    let duplicate_output = duplicate_command
        .output()
        .expect("duplicate rmus add-source should run");
    assert!(!duplicate_output.status.success());
    let duplicate_stderr = String::from_utf8(duplicate_output.stderr).unwrap();
    assert!(duplicate_stderr.contains("source path already exists"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_add_source_scan_warms_cache_without_launching_tui() {
    let state_dir = test_dir("cli-add-source-scan");
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("01 - First.flac"), "not real audio").unwrap();
    std::fs::write(music_dir.join("02 - Second.opus"), "not real audio").unwrap();

    let mut add_command = rmus_binary();
    state_env(
        add_command.args([
            "add-source",
            "Library",
            music_dir.to_str().unwrap(),
            "--scan",
        ]),
        &state_dir,
    );
    let output = add_command
        .output()
        .expect("rmus add-source --scan should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Added local source 'Library'"));
    assert!(stdout.contains("Scanned 2 local tracks from local source 'Library'"));

    let config = std::fs::read_to_string(isolated_config_path(&state_dir)).unwrap();
    assert!(config.contains("name = \"Library\""));

    let cache = std::fs::read_to_string(isolated_storage_path(&state_dir, "local cache"))
        .expect("add-source --scan should write the local metadata cache");
    assert!(cache.contains("01 - First.flac"));
    assert!(cache.contains("02 - Second.opus"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_remove_source_updates_config_without_launching_tui() {
    let state_dir = test_dir("cli-remove-source");
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("01 - First.flac"), "not real audio").unwrap();

    let mut add_command = rmus_binary();
    state_env(
        add_command.args(["add-source", "Library", music_dir.to_str().unwrap()]),
        &state_dir,
    );
    let add_output = add_command.output().expect("rmus add-source should run");
    assert!(add_output.status.success());

    let mut remove_command = rmus_binary();
    state_env(
        remove_command.args(["remove-source", "library"]),
        &state_dir,
    );
    let remove_output = remove_command
        .output()
        .expect("rmus remove-source should run");

    assert!(remove_output.status.success());
    assert!(remove_output.stderr.is_empty());
    let remove_stdout = String::from_utf8(remove_output.stdout).unwrap();
    assert!(remove_stdout.contains("Removed local source 'Library'"));
    assert!(remove_stdout.contains("0 configured sources"));

    let config_path = isolated_config_path(&state_dir);
    let config = std::fs::read_to_string(&config_path).unwrap();
    assert!(!config.contains("name = \"Library\""));

    let mut stats_command = rmus_binary();
    state_env(stats_command.arg("local-stats"), &state_dir);
    let stats_output = stats_command.output().expect("rmus local-stats should run");
    assert!(stats_output.status.success());
    let stats_stdout = String::from_utf8(stats_output.stdout).unwrap();
    assert!(stats_stdout.contains("No local sources configured"));

    let mut missing_command = rmus_binary();
    state_env(
        missing_command.args(["remove-source", "Library"]),
        &state_dir,
    );
    let missing_output = missing_command
        .output()
        .expect("missing rmus remove-source should run");
    assert!(!missing_output.status.success());
    let missing_stderr = String::from_utf8(missing_output.stderr).unwrap();
    assert!(missing_stderr.contains("source not found: Library"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_move_source_reorders_config_without_launching_tui() {
    let state_dir = test_dir("cli-move-source");
    let first_dir = state_dir.join("first");
    let second_dir = state_dir.join("second");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();

    let mut add_first = rmus_binary();
    state_env(
        add_first.args(["add-source", "First", first_dir.to_str().unwrap()]),
        &state_dir,
    );
    let add_first_output = add_first.output().expect("first add-source should run");
    assert!(add_first_output.status.success());

    let mut add_second = rmus_binary();
    state_env(
        add_second.args(["add-source", "Second", second_dir.to_str().unwrap()]),
        &state_dir,
    );
    let add_second_output = add_second.output().expect("second add-source should run");
    assert!(add_second_output.status.success());

    let mut move_command = rmus_binary();
    state_env(
        move_command.args(["move-source", "second", "up"]),
        &state_dir,
    );
    let move_output = move_command.output().expect("rmus move-source should run");

    assert!(move_output.status.success());
    assert!(move_output.stderr.is_empty());
    let move_stdout = String::from_utf8(move_output.stdout).unwrap();
    assert!(move_stdout.contains("Moved local source 'Second' from 2 to 1 of 2"));

    let config_text = std::fs::read_to_string(isolated_config_path(&state_dir)).unwrap();
    let config: Config = toml::from_str(&config_text).unwrap();
    assert_eq!(config.local.sources[0].name, "Second");
    assert_eq!(config.local.sources[1].name, "First");

    let mut boundary_command = rmus_binary();
    state_env(
        boundary_command.args(["move-source", "Second", "up"]),
        &state_dir,
    );
    let boundary_output = boundary_command
        .output()
        .expect("boundary rmus move-source should run");
    assert!(boundary_output.status.success());
    let boundary_stdout = String::from_utf8(boundary_output.stdout).unwrap();
    assert!(boundary_stdout.contains("Local source 'Second' already at 1 of 2"));

    let mut missing_command = rmus_binary();
    state_env(
        missing_command.args(["move-source", "Missing", "top"]),
        &state_dir,
    );
    let missing_output = missing_command
        .output()
        .expect("missing rmus move-source should run");
    assert!(!missing_output.status.success());
    let missing_stderr = String::from_utf8(missing_output.stderr).unwrap();
    assert!(missing_stderr.contains("source not found: Missing"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_list_sources_reports_configured_sources_without_launching_tui() {
    let state_dir = test_dir("cli-list-sources");
    let music_dir = state_dir.join("music");
    let missing_dir = state_dir.join("missing");
    std::fs::create_dir_all(&music_dir).unwrap();

    let config_path = isolated_config_path(&state_dir);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        format!(
            "\
[[local.sources]]
name = \"Library\"
path = \"{}\"

[[local.sources]]
name = \"Missing\"
path = \"{}\"

[audio]
default_volume = 50
",
            music_dir.to_string_lossy().replace('\\', "\\\\"),
            missing_dir.to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("list-sources"), &state_dir);
    let output = command.output().expect("rmus list-sources should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Local sources (2):"));
    assert!(stdout.contains("Library"));
    assert!(stdout.contains(&music_dir.to_string_lossy().to_string()));
    assert!(stdout.contains("[ok]"));
    assert!(stdout.contains("Missing"));
    assert!(stdout.contains(&missing_dir.to_string_lossy().to_string()));
    assert!(stdout.contains("[missing]"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_list_playlists_reports_saved_playlists_without_launching_tui() {
    let state_dir = test_dir("cli-list-playlists");
    std::fs::create_dir_all(&state_dir).unwrap();
    let playlists_dir = isolated_playlists_dir(&state_dir);
    std::fs::create_dir_all(&playlists_dir).unwrap();
    std::fs::write(
        playlists_dir.join("Road Mix.toml"),
        r#"
name = "Road Mix"

[[tracks]]
title = "First"

[[tracks]]
title = "Second"
"#,
    )
    .unwrap();
    std::fs::write(
        playlists_dir.join("Favorites.toml"),
        r#"
name = "Favorites"

[[tracks]]
title = "Only"
"#,
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("list-playlists"), &state_dir);
    let output = command.output().expect("rmus list-playlists should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Playlists (2):"));
    assert!(stdout.contains("- Favorites (1 track)"));
    assert!(stdout.contains("- Road Mix (2 tracks)"));

    let mut empty_command = rmus_binary();
    let empty_dir = test_dir("cli-list-playlists-empty");
    std::fs::create_dir_all(&empty_dir).unwrap();
    state_env(empty_command.arg("list-playlists"), &empty_dir);
    let empty_output = empty_command
        .output()
        .expect("empty rmus list-playlists should run");
    assert!(empty_output.status.success());
    assert!(empty_output.stderr.is_empty());
    let empty_stdout = String::from_utf8(empty_output.stdout).unwrap();
    assert!(empty_stdout.contains("No playlists found"));
    assert!(empty_stdout.contains("rmus import-playlist"));

    let _ = std::fs::remove_dir_all(state_dir);
    let _ = std::fs::remove_dir_all(empty_dir);
}

#[test]
fn test_cli_show_playlist_reports_saved_tracks_without_launching_tui() {
    let state_dir = test_dir("cli-show-playlist");
    std::fs::create_dir_all(&state_dir).unwrap();
    let playlists_dir = isolated_playlists_dir(&state_dir);
    std::fs::create_dir_all(&playlists_dir).unwrap();
    std::fs::write(
        playlists_dir.join("Road Mix.toml"),
        r#"
name = "Road Mix"

[[tracks]]
title = "Local Song"
artist = "Local Artist"
album_name = "Local Album"
path = "/music/local.flac"

[[tracks]]
title = "Stream Song"
artist = "Stream Artist"
album_name = "Stream Album"
stream_service = "Qobuz"
stream_track_id = "qbz-1"
"#,
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.args(["show-playlist", "road mix"]), &state_dir);
    let output = command.output().expect("rmus show-playlist should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Playlist 'Road Mix' (2 tracks)"));
    assert!(stdout.contains("1. Local Artist - Local Song (Local Album) [local] /music/local.flac"));
    assert!(stdout.contains("2. Stream Artist - Stream Song (Stream Album) [Qobuz: qbz-1]"));

    let mut limited_command = rmus_binary();
    state_env(
        limited_command.args(["show-playlist", "road mix", "--limit", "1"]),
        &state_dir,
    );
    let limited_output = limited_command
        .output()
        .expect("rmus show-playlist --limit should run");
    assert!(limited_output.status.success());
    assert!(limited_output.stderr.is_empty());
    let limited_stdout = String::from_utf8(limited_output.stdout).unwrap();
    assert!(limited_stdout.contains("Playlist 'Road Mix' (2 tracks)"));
    assert!(limited_stdout
        .contains("1. Local Artist - Local Song (Local Album) [local] /music/local.flac"));
    assert!(!limited_stdout.contains("Stream Song"));
    assert!(limited_stdout.contains("... 1 more track; rerun with --limit 2 to show all"));

    let mut missing_command = rmus_binary();
    state_env(
        missing_command.args(["show-playlist", "Missing"]),
        &state_dir,
    );
    let missing_output = missing_command
        .output()
        .expect("missing rmus show-playlist should run");
    assert!(!missing_output.status.success());
    let stderr = String::from_utf8(missing_output.stderr).unwrap();
    assert!(stderr.contains("Playlist 'Missing' not found"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_delete_playlist_removes_saved_playlist_without_launching_tui() {
    let state_dir = test_dir("cli-delete-playlist");
    std::fs::create_dir_all(&state_dir).unwrap();
    let playlists_dir = isolated_playlists_dir(&state_dir);
    std::fs::create_dir_all(&playlists_dir).unwrap();
    std::fs::write(
        playlists_dir.join("Road Mix.toml"),
        r#"
name = "Road Mix"

[[tracks]]
title = "First"

[[tracks]]
title = "Second"
"#,
    )
    .unwrap();
    std::fs::write(
        playlists_dir.join("Keep.toml"),
        r#"
name = "Keep"
tracks = []
"#,
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.args(["delete-playlist", "road mix"]), &state_dir);
    let output = command.output().expect("rmus delete-playlist should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Deleted playlist 'Road Mix' (2 tracks)"));
    assert!(!playlists_dir.join("Road Mix.toml").exists());
    assert!(playlists_dir.join("Keep.toml").exists());

    let mut list_command = rmus_binary();
    state_env(list_command.arg("list-playlists"), &state_dir);
    let list_output = list_command
        .output()
        .expect("rmus list-playlists should run after delete");
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8(list_output.stdout).unwrap();
    assert!(list_stdout.contains("- Keep (0 tracks)"));
    assert!(!list_stdout.contains("Road Mix"));

    let mut missing_command = rmus_binary();
    state_env(
        missing_command.args(["delete-playlist", "Missing"]),
        &state_dir,
    );
    let missing_output = missing_command
        .output()
        .expect("missing rmus delete-playlist should run");
    assert!(!missing_output.status.success());
    let stderr = String::from_utf8(missing_output.stderr).unwrap();
    assert!(stderr.contains("Playlist 'Missing' not found"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_local_stats_reports_configured_library_without_launching_tui() {
    let state_dir = test_dir("cli-local-stats");
    let music_dir = state_dir.join("music").join("Album");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("01 - First.flac"), "not real audio").unwrap();
    std::fs::write(music_dir.join("02 - Second.opus"), "not real audio").unwrap();
    std::fs::write(music_dir.join("cover.jpg"), "not audio").unwrap();

    let config_path = isolated_config_path(&state_dir);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        format!(
            "\
[[local.sources]]
name = \"Library\"
path = \"{}\"

[audio]
default_volume = 50
",
            state_dir
                .join("music")
                .to_string_lossy()
                .replace('\\', "\\\\")
        ),
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("local-stats"), &state_dir);
    let output = command.output().expect("rmus local-stats should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Local library: 1 configured source"));
    assert!(stdout.contains("0 missing"));
    assert!(stdout.contains("1 discovered album"));
    assert!(stdout.contains("2 playable tracks"));
    assert!(stdout.contains("album discovery: complete"));
    assert!(stdout.contains("local-cache.toml"));
    assert!(stdout.contains("(missing)"));
    assert!(
        find_file_named(&state_dir, "local-cache.toml").is_none(),
        "local-stats should not warm or write the local metadata cache"
    );

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_scan_local_warms_cache_without_launching_tui() {
    let state_dir = test_dir("cli-scan-local");
    let music_dir = state_dir.join("music").join("Album");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("01 - First.flac"), "not real audio").unwrap();
    std::fs::write(music_dir.join("02 - Second.opus"), "not real audio").unwrap();

    let config_path = isolated_config_path(&state_dir);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        format!(
            "\
[[local.sources]]
name = \"Library\"
path = \"{}\"

[audio]
default_volume = 50
",
            state_dir
                .join("music")
                .to_string_lossy()
                .replace('\\', "\\\\")
        ),
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("scan-local"), &state_dir);
    let output = command.output().expect("rmus scan-local should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Scanned 2 local tracks from 1 local source"));
    assert!(stdout.contains("local-cache.toml"));

    let cache_path = find_file_named(&state_dir, "local-cache.toml")
        .expect("scan-local should write the local cache under the isolated state dir");
    let cache = std::fs::read_to_string(cache_path).unwrap();
    assert!(cache.contains("01 - First.flac"));
    assert!(cache.contains("02 - Second.opus"));
    assert!(cache.contains("[[album_discoveries]]"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_scan_local_can_target_named_source_without_launching_tui() {
    let state_dir = test_dir("cli-scan-local-source");
    let library_dir = state_dir.join("library").join("Album");
    let archive_dir = state_dir.join("archive").join("Album");
    std::fs::create_dir_all(&library_dir).unwrap();
    std::fs::create_dir_all(&archive_dir).unwrap();
    std::fs::write(library_dir.join("01 - Library.flac"), "not real audio").unwrap();
    std::fs::write(archive_dir.join("01 - Archive.opus"), "not real audio").unwrap();

    let config_path = isolated_config_path(&state_dir);
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        format!(
            "\
[[local.sources]]
name = \"Library\"
path = \"{}\"

[[local.sources]]
name = \"Archive\"
path = \"{}\"

[audio]
default_volume = 50
",
            state_dir
                .join("library")
                .to_string_lossy()
                .replace('\\', "\\\\"),
            state_dir
                .join("archive")
                .to_string_lossy()
                .replace('\\', "\\\\")
        ),
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(command.args(["scan-local", "archive"]), &state_dir);
    let output = command
        .output()
        .expect("rmus scan-local archive should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Scanned 1 local track from local source 'Archive'"));
    assert!(stdout.contains("local-cache.toml"));

    let cache_path = find_file_named(&state_dir, "local-cache.toml")
        .expect("targeted scan-local should write the local cache");
    let cache = std::fs::read_to_string(cache_path).unwrap();
    assert!(cache.contains("01 - Archive.opus"));
    assert!(!cache.contains("01 - Library.flac"));

    let mut missing_command = rmus_binary();
    state_env(missing_command.args(["scan-local", "Missing"]), &state_dir);
    let missing_output = missing_command
        .output()
        .expect("rmus scan-local Missing should run");
    assert!(!missing_output.status.success());
    let stderr = String::from_utf8(missing_output.stderr).unwrap();
    assert!(stderr.contains("source not found: Missing"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_clear_cache_runs_without_launching_tui() {
    let state_dir = test_dir("cli-clear-cache");
    std::fs::create_dir_all(&state_dir).unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("clear-cache"), &state_dir);
    let output = command.output().expect("rmus clear-cache should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Local cache already absent"));
    assert!(stdout.contains("local-cache.toml"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_show_history_prints_saved_history_without_launching_tui() {
    let state_dir = test_dir("cli-show-history");
    let config_dir = state_dir.join("rmus-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    HistoryStore::with_path(config_dir.join("history.toml"))
        .save(&[
            Song {
                title: "Local History".to_string(),
                artist: "History Artist".to_string(),
                album_name: "History Album".to_string(),
                path: PathBuf::from("/music/history.flac"),
                ..Default::default()
            },
            Song {
                title: "Stream History".to_string(),
                artist: "Stream Artist".to_string(),
                album_name: "Stream Album".to_string(),
                stream_service: Some("Tidal".to_string()),
                stream_track_id: Some("tidal-1".to_string()),
                ..Default::default()
            },
        ])
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("show-history"), &state_dir);
    let output = command.output().expect("rmus show-history should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Recently played (2 tracks)"));
    assert!(stdout
        .contains("1. History Artist - Local History (History Album) [local] /music/history.flac"));
    assert!(stdout.contains("2. Stream Artist - Stream History (Stream Album) [Tidal: tidal-1]"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_show_history_limit_truncates_saved_history_without_launching_tui() {
    let state_dir = test_dir("cli-show-history-limit");
    let config_dir = state_dir.join("rmus-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    HistoryStore::with_path(config_dir.join("history.toml"))
        .save(&[
            Song {
                title: "First History".to_string(),
                artist: "History Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second History".to_string(),
                artist: "History Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ])
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.args(["show-history", "--limit", "1"]), &state_dir);
    let output = command.output().expect("rmus show-history should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Recently played (2 tracks)"));
    assert!(stdout.contains("1. History Artist - First History [local] /music/first.flac"));
    assert!(!stdout.contains("Second History"));
    assert!(stdout.contains("... 1 more track; rerun with --limit 2 to show all"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_show_queue_prints_saved_queue_without_launching_tui() {
    let state_dir = test_dir("cli-show-queue");
    let config_dir = state_dir.join("rmus-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    QueueStore::with_path(config_dir.join("queue.toml"))
        .save(&QueueState::new(
            vec![
                Song {
                    title: "First Queue".to_string(),
                    artist: "Queue Artist".to_string(),
                    path: PathBuf::from("/music/first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second Queue".to_string(),
                    artist: "Queue Artist".to_string(),
                    path: PathBuf::from("/music/second.flac"),
                    ..Default::default()
                },
            ],
            1,
        ))
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("show-queue"), &state_dir);
    let output = command.output().expect("rmus show-queue should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Saved queue (2 tracks, position 2 of 2)"));
    assert!(stdout.contains("  1. Queue Artist - First Queue [local] /music/first.flac"));
    assert!(stdout.contains("> 2. Queue Artist - Second Queue [local] /music/second.flac"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_show_queue_limit_truncates_saved_queue_without_launching_tui() {
    let state_dir = test_dir("cli-show-queue-limit");
    let config_dir = state_dir.join("rmus-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    QueueStore::with_path(config_dir.join("queue.toml"))
        .save(&QueueState::new(
            vec![
                Song {
                    title: "First Queue".to_string(),
                    artist: "Queue Artist".to_string(),
                    path: PathBuf::from("/music/first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second Queue".to_string(),
                    artist: "Queue Artist".to_string(),
                    path: PathBuf::from("/music/second.flac"),
                    ..Default::default()
                },
            ],
            1,
        ))
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.args(["show-queue", "--limit", "1"]), &state_dir);
    let output = command.output().expect("rmus show-queue should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Saved queue (2 tracks, position 2 of 2)"));
    assert!(stdout.contains("  1. Queue Artist - First Queue [local] /music/first.flac"));
    assert!(!stdout.contains("Second Queue"));
    assert!(stdout.contains("... 1 more track; rerun with --limit 2 to show all"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_clear_history_removes_saved_history_without_launching_tui() {
    let state_dir = test_dir("cli-clear-history");
    let config_dir = state_dir.join("rmus-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let history_path = config_dir.join("history.toml");
    HistoryStore::with_path(history_path.clone())
        .save(&[
            Song {
                title: "First History".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second History".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ])
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("clear-history"), &state_dir);
    let output = command.output().expect("rmus clear-history should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Removed history (2 tracks)"));
    assert!(stdout.contains("history.toml"));
    assert!(!history_path.exists());

    let mut command = rmus_binary();
    state_env(command.arg("clear-history"), &state_dir);
    let output = command
        .output()
        .expect("second rmus clear-history should run");
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("History already absent"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_clear_queue_removes_saved_queue_without_launching_tui() {
    let state_dir = test_dir("cli-clear-queue");
    let config_dir = state_dir.join("rmus-config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let queue_path = config_dir.join("queue.toml");
    QueueStore::with_path(queue_path.clone())
        .save(&QueueState::new(
            vec![
                Song {
                    title: "First Queue".to_string(),
                    path: PathBuf::from("/music/first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second Queue".to_string(),
                    path: PathBuf::from("/music/second.flac"),
                    ..Default::default()
                },
            ],
            1,
        ))
        .unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("clear-queue"), &state_dir);
    let output = command.output().expect("rmus clear-queue should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Removed saved queue (2 tracks)"));
    assert!(stdout.contains("queue.toml"));
    assert!(!queue_path.exists());

    let mut command = rmus_binary();
    state_env(command.arg("clear-queue"), &state_dir);
    let output = command
        .output()
        .expect("second rmus clear-queue should run");
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("Saved queue already absent"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_import_playlist_writes_rmus_playlist() {
    let state_dir = test_dir("cli-import-playlist");
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::write(music_dir.join("song.flac"), "not real audio").unwrap();
    let m3u = music_dir.join("mix.m3u");
    std::fs::write(
        &m3u,
        "\
#EXTM3U
#EXTINF:245,Artist - Song
song.flac
",
    )
    .unwrap();

    let mut command = rmus_binary();
    state_env(
        command.args(["import-playlist", m3u.to_str().unwrap(), "Imported Mix"]),
        &state_dir,
    );
    let output = command.output().expect("rmus import-playlist should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Imported 1 track into playlist 'Imported Mix'"));

    let playlist_path = find_file_named(&state_dir, "Imported Mix.toml")
        .expect("import should create a playlist file under the isolated state dir");
    let playlist = std::fs::read_to_string(playlist_path).unwrap();
    let playlist: Playlist = toml::from_str(&playlist).unwrap();
    assert_eq!(playlist.name, "Imported Mix");
    assert_eq!(playlist.tracks.len(), 1);
    let track = &playlist.tracks[0];
    assert_eq!(track.title, "Artist - Song");
    assert_eq!(track.duration_secs, Some(245.0));
    let expected_path = music_dir.join("song.flac").to_string_lossy().into_owned();
    assert_eq!(track.path.as_deref(), Some(expected_path.as_str()));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_export_playlist_writes_m3u8() {
    let state_dir = test_dir("cli-export-playlist");
    let music_dir = state_dir.join("music");
    std::fs::create_dir_all(&music_dir).unwrap();
    let first = music_dir.join("first.flac");
    std::fs::write(&first, "not real audio").unwrap();
    let m3u = music_dir.join("mix.m3u");
    std::fs::write(
        &m3u,
        "\
#EXTM3U
#EXTINF:200,Artist - First
first.flac
",
    )
    .unwrap();
    let mut import_command = rmus_binary();
    state_env(
        import_command.args(["import-playlist", m3u.to_str().unwrap(), "Export Me"]),
        &state_dir,
    );
    let import_output = import_command
        .output()
        .expect("rmus import-playlist should run");
    assert!(import_output.status.success());

    let export_path = state_dir.join("exported.m3u8");
    let mut export_command = rmus_binary();
    state_env(
        export_command.args([
            "export-playlist",
            "Export Me",
            export_path.to_str().unwrap(),
        ]),
        &state_dir,
    );
    let export_output = export_command
        .output()
        .expect("rmus export-playlist should run");

    assert!(export_output.status.success());
    assert!(export_output.stderr.is_empty());
    let stdout = String::from_utf8(export_output.stdout).unwrap();
    assert!(stdout.contains("Exported 1 track from playlist 'Export Me'"));
    let exported = std::fs::read_to_string(export_path).unwrap();
    assert_eq!(
        exported,
        format!(
            "\
#EXTM3U
#EXTINF:200,Artist - First
{}
",
            first.to_string_lossy()
        )
    );

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_doctor_reports_runtime_checks_without_launching_tui() {
    let state_dir = test_dir("cli-doctor");
    std::fs::create_dir_all(&state_dir).unwrap();
    let path = std::env::join_paths([fake_mpv_bin_dir("cli-doctor-mpv")]).unwrap();

    let mut command = rmus_binary();
    state_env(command.arg("doctor").env("PATH", path), &state_dir);
    let output = command.output().expect("rmus doctor should run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("rmus doctor"));
    assert!(stdout.contains("[ok] version:"));
    assert!(stdout.contains("[ok] mpv:"));
    assert!(stdout.contains("[warn] config:"));
    assert!(stdout.contains("[warn] local sources: config missing"));

    let _ = std::fs::remove_dir_all(state_dir);
}

#[test]
fn test_cli_unknown_flag_exits_with_usage() {
    let output = rmus_binary()
        .arg("--unknown")
        .output()
        .expect("rmus unknown flag should run");

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown argument '--unknown'"));
    assert!(stderr.contains("Usage:"));
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
    track_results: Vec<StreamTrack>,
    polls_until_ready: usize,
    poll_count: usize,
    search_delay_ms: u64,
    query_album_results: HashMap<String, Vec<StreamAlbum>>,
    query_delays_ms: HashMap<String, u64>,
    stream_urls: HashMap<String, String>,
    missing_stream_ids: HashSet<String>,
}

impl MockStreamingService {
    fn new_authenticated(name: &str, albums: Vec<StreamAlbum>) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: true,
            album_results: albums,
            album_tracks: mock_album_tracks(),
            track_results: Vec::new(),
            polls_until_ready: 0,
            poll_count: 0,
            search_delay_ms: 0,
            query_album_results: HashMap::new(),
            query_delays_ms: HashMap::new(),
            stream_urls: HashMap::new(),
            missing_stream_ids: HashSet::new(),
        }
    }

    fn new_pending(name: &str, polls_needed: usize, albums: Vec<StreamAlbum>) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: false,
            album_results: albums,
            album_tracks: mock_album_tracks(),
            track_results: Vec::new(),
            polls_until_ready: polls_needed,
            poll_count: 0,
            search_delay_ms: 0,
            query_album_results: HashMap::new(),
            query_delays_ms: HashMap::new(),
            stream_urls: HashMap::new(),
            missing_stream_ids: HashSet::new(),
        }
    }

    fn new_authenticated_slow(name: &str, albums: Vec<StreamAlbum>, delay_ms: u64) -> Self {
        Self {
            service_name: name.to_string(),
            authenticated: true,
            album_results: albums,
            album_tracks: mock_album_tracks(),
            track_results: Vec::new(),
            polls_until_ready: 0,
            poll_count: 0,
            search_delay_ms: delay_ms,
            query_album_results: HashMap::new(),
            query_delays_ms: HashMap::new(),
            stream_urls: HashMap::new(),
            missing_stream_ids: HashSet::new(),
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

    fn with_stream_url(mut self, track_id: &str, url: &str) -> Self {
        self.stream_urls
            .insert(track_id.to_string(), url.to_string());
        self
    }

    fn with_missing_stream_url(mut self, track_id: &str) -> Self {
        self.missing_stream_ids.insert(track_id.to_string());
        self
    }

    fn with_track_results(mut self, tracks: Vec<StreamTrack>) -> Self {
        self.track_results = tracks;
        self
    }

    fn with_album_tracks(mut self, tracks: Vec<StreamTrack>) -> Self {
        self.album_tracks = tracks;
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

    fn persist_data(&self) -> Option<String> {
        if !self.authenticated {
            return None;
        }

        serde_json::to_string(&TidalConfig {
            access_token: "mock-access-token".to_string(),
            refresh_token: "mock-refresh-token".to_string(),
            country_code: "US".to_string(),
            token_expiry: 1_900_000_000,
        })
        .ok()
    }

    fn search(
        &mut self,
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        Ok(self.track_results.clone())
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
        track_id: &str,
    ) -> Result<Option<ResolvedStream>, Box<dyn std::error::Error>> {
        if self.missing_stream_ids.contains(track_id) {
            return Ok(None);
        }

        let url = self
            .stream_urls
            .get(track_id)
            .cloned()
            .unwrap_or_else(|| "https://example.com/stream.flac".to_string());
        Ok(Some(ResolvedStream {
            source: ResolvedStreamSource::Url(url),
            quality_label: Some("Hi-Res".to_string()),
        }))
    }
}

#[derive(Debug, Default)]
struct MockPlayer {
    played: Arc<Mutex<Vec<Song>>>,
    enqueued: Arc<Mutex<Vec<Song>>>,
    queue: Vec<Song>,
    queue_position: usize,
    info: PlaybackInfo,
}

type SharedSongLog = Arc<Mutex<Vec<Song>>>;
type MockPlayerSetup = (MockPlayer, SharedSongLog, SharedSongLog);

impl MockPlayer {
    fn new() -> MockPlayerSetup {
        let played = Arc::new(Mutex::new(Vec::new()));
        let enqueued = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                played: played.clone(),
                enqueued: enqueued.clone(),
                ..Default::default()
            },
            played,
            enqueued,
        )
    }

    fn sync_info_queue(&mut self) {
        self.info.queue_len = self.queue.len();
        self.info.queue_position = if self.queue.is_empty() {
            0
        } else {
            self.queue_position.min(self.queue.len() - 1)
        };
    }
}

impl MusicPlayer for MockPlayer {
    fn play(&mut self, song: &Song) -> PlayerResult<()> {
        self.played.lock().unwrap().push(song.clone());
        self.queue = vec![song.clone()];
        self.queue_position = 0;
        self.info.current_song = Some(song.clone());
        self.info.state = PlaybackState::Playing;
        self.info.position = 0.0;
        self.sync_info_queue();
        Ok(())
    }

    fn play_album(&mut self, songs: Vec<Song>, start_index: usize) -> PlayerResult<()> {
        self.queue = songs;
        self.queue_position = start_index;
        if let Some(song) = self.queue.get(start_index).cloned() {
            self.played.lock().unwrap().push(song.clone());
            self.info.current_song = Some(song);
            self.info.state = PlaybackState::Playing;
            self.info.position = 0.0;
        }
        self.sync_info_queue();
        Ok(())
    }

    fn toggle_pause(&mut self) -> PlayerResult<()> {
        self.info.state = match self.info.state {
            PlaybackState::Playing => PlaybackState::Paused,
            PlaybackState::Paused => PlaybackState::Playing,
            PlaybackState::Stopped => PlaybackState::Stopped,
        };
        Ok(())
    }

    fn stop(&mut self) -> PlayerResult<()> {
        self.info.state = PlaybackState::Stopped;
        self.info.current_song = None;
        self.info.position = 0.0;
        self.sync_info_queue();
        Ok(())
    }

    fn next(&mut self) -> PlayerResult<()> {
        if self.queue_position + 1 < self.queue.len() {
            self.queue_position += 1;
            self.info.current_song = self.queue.get(self.queue_position).cloned();
            self.info.state = PlaybackState::Playing;
            self.info.position = 0.0;
        }
        self.sync_info_queue();
        Ok(())
    }

    fn previous(&mut self) -> PlayerResult<()> {
        if self.queue_position > 0 {
            self.queue_position -= 1;
            self.info.current_song = self.queue.get(self.queue_position).cloned();
            self.info.state = PlaybackState::Playing;
            self.info.position = 0.0;
        }
        self.sync_info_queue();
        Ok(())
    }

    fn seek(&mut self, position: f64) -> PlayerResult<()> {
        self.info.position = position;
        Ok(())
    }

    fn set_volume(&mut self, volume: u8) -> PlayerResult<()> {
        self.info.volume = volume;
        Ok(())
    }

    fn poll(&mut self) -> PlayerResult<PlaybackInfo> {
        self.sync_info_queue();
        Ok(self.info.clone())
    }

    fn get_playback_info(&self) -> &PlaybackInfo {
        &self.info
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn shutdown(&mut self) -> PlayerResult<()> {
        Ok(())
    }

    fn toggle_shuffle(&mut self) -> PlayerResult<()> {
        self.info.shuffle = match self.info.shuffle {
            ShuffleMode::Off => ShuffleMode::On,
            ShuffleMode::On => ShuffleMode::Off,
        };
        Ok(())
    }

    fn cycle_repeat(&mut self) -> PlayerResult<()> {
        self.info.repeat = self.info.repeat.cycle();
        Ok(())
    }

    fn enqueue(&mut self, songs: Vec<Song>) -> PlayerResult<()> {
        self.enqueued.lock().unwrap().extend(songs.clone());
        self.queue.extend(songs);
        self.sync_info_queue();
        Ok(())
    }

    fn restore_queue(&mut self, songs: Vec<Song>, position: usize) -> PlayerResult<()> {
        self.queue = songs;
        self.queue_position = if self.queue.is_empty() {
            0
        } else {
            position.min(self.queue.len() - 1)
        };
        self.info.current_song = None;
        self.info.state = PlaybackState::Stopped;
        self.info.position = 0.0;
        self.sync_info_queue();
        Ok(())
    }

    fn get_queue(&self) -> &[Song] {
        &self.queue
    }

    fn get_queue_position(&self) -> usize {
        self.queue_position
    }

    fn remove_from_queue(&mut self, index: usize) -> PlayerResult<()> {
        if index < self.queue.len() {
            self.queue.remove(index);
        }
        self.sync_info_queue();
        Ok(())
    }

    fn move_in_queue(&mut self, from: usize, to: usize) -> PlayerResult<()> {
        if from < self.queue.len() && to < self.queue.len() && from != to {
            let song = self.queue.remove(from);
            self.queue.insert(to, song);
        }
        self.sync_info_queue();
        Ok(())
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
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();

    assert_eq!(frame.buffer.cell((0, 0)).unwrap().bg, theme::BACKGROUND);
}

#[test]
fn test_initial_center_panel_shows_guidance() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(420, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Select an album or playlist"),
        "initial center panel should explain what to open first"
    );
    assert!(
        text.contains("Use / to search"),
        "initial center panel should point users toward search"
    );
}

#[test]
fn test_search_input_renders_cursor_at_current_position() {
    let mut app = make_app(None, None);
    app.focused_window = FocusedWindow::Center;
    app.center_panel.open_search();
    for c in "album".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Home));

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("> _album"),
        "search input should render the cursor at the current cursor position"
    );
    assert!(
        !text.contains("> album_"),
        "search input should not always render the cursor at the end"
    );
}

#[test]
fn test_empty_selected_album_and_playlist_show_specific_guidance() {
    let local_dir = test_dir("empty-local-source");
    std::fs::create_dir_all(&local_dir).unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Empty Library".to_string(),
                path: local_dir.clone(),
            }],
        },
        ..default_config()
    };
    let playlist_dir = test_dir("empty-playlist-guidance");
    let store = PlaylistStore::with_dir(playlist_dir.clone());
    store.create("Quiet Mix".to_string()).unwrap();
    let mut app = App::new_for_test_with_playlist_store(config, None, None, store.clone());
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("No songs found"),
        "empty selected local source should explain that no playable files were found"
    );
    assert!(
        text.contains("Check this source folder"),
        "empty selected local source should point users back to the folder"
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Playlist is empty"),
        "empty selected playlist should explain that the playlist has no tracks"
    );
    assert!(
        text.contains("Add songs with A"),
        "empty selected playlist should point users to the add-to-playlist shortcut"
    );

    let _ = std::fs::remove_dir_all(local_dir);
    let _ = std::fs::remove_dir_all(playlist_dir);
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
fn test_empty_left_tabs_render_contextual_guidance() {
    let mut app = make_app(None, None);
    let render_text = |app: &mut App| {
        let backend = TestBackend::new(220, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let frame = terminal.draw(|frame| app.render(frame)).unwrap();
        extract_buffer_text(frame.buffer)
    };

    let text = render_text(&mut app);
    assert!(
        text.contains("No local sources"),
        "empty Local tab should show visible setup guidance"
    );
    assert!(
        text.contains("Open Settings"),
        "empty Local tab should point users to Settings"
    );

    switch_to_tab(&mut app, "Playlists");
    let text = render_text(&mut app);
    assert!(
        text.contains("No playlists yet"),
        "empty Playlists tab should show visible playlist guidance"
    );
    assert!(
        text.contains("Create one"),
        "empty Playlists tab should point users toward playlist creation"
    );

    switch_to_tab(&mut app, "Qobuz");
    let text = render_text(&mut app);
    assert!(
        text.contains("Use / to search Qobuz"),
        "empty Qobuz tab should point users to search"
    );

    switch_to_tab(&mut app, "Tidal");
    let text = render_text(&mut app);
    assert!(
        text.contains("Use / to search Tidal"),
        "empty Tidal tab should point users to search"
    );
}

#[test]
fn test_initial_local_source_can_be_selected_without_cursor_movement() {
    let dir = test_dir("initial-local-source");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Ready.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);

    app.execute(Action::SelectAlbum);

    let songs = app.center_panel.get_songs();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].title, "01 - Ready.flac");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_local_tab_discovers_album_folders_under_source() {
    let dir = test_dir("local-album-folders");
    std::fs::create_dir_all(dir.join("Alpha")).unwrap();
    std::fs::create_dir_all(dir.join("Beta")).unwrap();
    std::fs::write(dir.join("Alpha").join("01 - First.flac"), "").unwrap();
    std::fs::write(dir.join("Beta").join("01 - Second.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Alpha"),
        "local child album folder should appear in the left panel"
    );
    assert!(
        text.contains("Beta"),
        "second local child album folder should appear in the left panel"
    );
    assert!(
        text.contains("All Local Tracks"),
        "local tab should offer a whole-library collection when multiple albums are available"
    );

    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("01 - First.flac"));
    assert!(
        text.contains("01 - Second.flac"),
        "opening All Local Tracks should show tracks from sibling albums"
    );

    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("01 - First.flac"));
    assert!(
        !text.contains("01 - Second.flac"),
        "opening the first discovered local album should not show tracks from sibling albums"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_local_source_entry_excludes_child_album_tracks() {
    let dir = test_dir("local-root-and-child-albums");
    std::fs::create_dir_all(dir.join("Child Album")).unwrap();
    std::fs::write(dir.join("00 - Root Track.flac"), "").unwrap();
    std::fs::write(dir.join("Child Album").join("01 - Child Track.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("00 - Root Track.flac"));
    assert!(
        !text.contains("01 - Child Track.flac"),
        "opening the source-root entry should not include discovered child album tracks"
    );

    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("01 - Child Track.flac"));
    assert!(
        !text.contains("00 - Root Track.flac"),
        "opening the child album entry should not include source-root tracks"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_left_panel_play_selected_local_album_starts_collection() {
    let dir = test_dir("left-play-local-collection");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - First.flac"), "").unwrap();
    std::fs::write(dir.join("02 - Second.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        config,
        None,
        None,
        PlaylistStore::with_dir(test_dir("left-play-local-collection-playlists")),
        Box::new(player),
    );

    app.execute(Action::PlaySelectedCollection);

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "01 - First.flac");
    drop(played);

    app.execute(Action::ShowQueue);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Queue (2 tracks)"));
    assert!(text.contains("> 1. 01 - First.flac"));
    assert!(text.contains("  2. 02 - Second.flac"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_left_panel_play_all_local_tracks_starts_full_library() {
    let dir = test_dir("left-play-all-local-tracks");
    std::fs::create_dir_all(dir.join("Alpha")).unwrap();
    std::fs::create_dir_all(dir.join("Beta")).unwrap();
    std::fs::write(dir.join("Alpha").join("01 - Alpha.flac"), "").unwrap();
    std::fs::write(dir.join("Beta").join("01 - Beta.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        config,
        None,
        None,
        PlaylistStore::with_dir(test_dir("left-play-all-local-tracks-playlists")),
        Box::new(player),
    );

    assert_eq!(
        app.left_panel.selected_item_label().as_deref(),
        Some("All Local Tracks")
    );
    app.execute(Action::PlaySelectedCollection);

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "01 - Alpha.flac");
    drop(played);

    app.execute(Action::ShowQueue);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Queue (2 tracks)"));
    assert!(text.contains("> 1. 01 - Alpha.flac"));
    assert!(text.contains("  2. 01 - Beta.flac"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_left_panel_favorites_selected_local_album() {
    let dir = test_dir("left-favorite-local-collection");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - First.flac"), "").unwrap();
    std::fs::write(dir.join("02 - Second.flac"), "").unwrap();
    let playlist_dir = test_dir("left-favorite-local-collection-playlists");
    let store = PlaylistStore::with_dir(playlist_dir.clone());

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test_with_playlist_store(config, None, None, store.clone());

    dispatch_key(&mut app, make_key(KeyCode::Char('F')));
    dispatch_key(&mut app, make_key(KeyCode::Char('F')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert_eq!(playlists[0].tracks.len(), 2);
    assert_eq!(playlists[0].tracks[0].title, "01 - First.flac");
    assert_eq!(playlists[0].tracks[1].title, "02 - Second.flac");

    dispatch_key(&mut app, make_key(KeyCode::Char('U')));
    dispatch_key(&mut app, make_key(KeyCode::Char('U')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert!(playlists[0].tracks.is_empty());

    let backend = TestBackend::new(520, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added 2 tracks to Favorites"),
        "favoriting a selected album should report the added track count"
    );
    assert!(
        text.contains("Selected tracks are already in Favorites"),
        "favoriting the same album twice should skip duplicates"
    );
    assert!(
        text.contains("Removed 2 tracks from Favorites"),
        "unfavoriting a selected album should report the removed track count"
    );
    assert!(
        text.contains("Selected tracks are not in Favorites"),
        "unfavoriting the same album twice should report missing Favorites entries"
    );

    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(playlist_dir);
}

#[test]
fn test_left_panel_queue_selected_playlist_enqueues_all_tracks() {
    let dir = test_dir("left-queue-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Road".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "First Song".to_string(),
                    artist: "First Artist".to_string(),
                    path: PathBuf::from("/music/first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second Song".to_string(),
                    artist: "Second Artist".to_string(),
                    path: PathBuf::from("/music/second.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();
    let (player, _played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::EnqueueSelectedCollection);
    app.tick();

    let enqueued = enqueued.lock().unwrap();
    assert_eq!(enqueued.len(), 2);
    assert_eq!(enqueued[0].title, "First Song");
    assert_eq!(enqueued[1].title, "Second Song");
    drop(enqueued);

    let backend = TestBackend::new(240, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Queued 2 tracks"),
        "queueing a playlist from the left panel should show collection feedback"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_added_local_source_can_be_opened_immediately() {
    let dir = test_dir("add-local-source-live");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Newly Added.flac"), "").unwrap();
    let mut app = make_app(None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('a')));
    for c in "Fresh Library".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in dir.to_string_lossy().chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Saved"),
        "adding a local source should show settings feedback"
    );

    app.delegate_key_to_panel(make_key(KeyCode::Esc));
    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Fresh Library"),
        "newly added source should appear in the Local tab without restarting"
    );
    assert!(
        text.contains("01 - Newly Added.flac"),
        "newly added source should be selectable immediately"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_added_local_source_returns_focus_to_browsable_local_list() {
    let dir = test_dir("add-local-source-focus");
    std::fs::create_dir_all(dir.join("Alpha")).unwrap();
    std::fs::create_dir_all(dir.join("Beta")).unwrap();
    std::fs::write(dir.join("Alpha").join("01 - Alpha.flac"), "").unwrap();
    std::fs::write(dir.join("Beta").join("01 - Beta.flac"), "").unwrap();
    let mut app = make_app(None, None);
    app.focused_window = FocusedWindow::Center;

    dispatch_key(&mut app, make_key(KeyCode::Char('s')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('a')));
    for c in "Fresh Library".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in dir.to_string_lossy().chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();
    dispatch_key(&mut app, make_key(KeyCode::Esc));

    assert_eq!(app.focused_window, FocusedWindow::Left);
    assert_eq!(app.left_panel.active_tab_name(), "Local");

    dispatch_key(&mut app, make_key(KeyCode::Down));
    dispatch_key(&mut app, make_key(KeyCode::Down));
    dispatch_key(&mut app, make_key(KeyCode::Enter));

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("01 - Beta.flac"),
        "after adding a source, the Local list should be focused so users can scroll and open it"
    );
    assert!(
        !text.contains("01 - Alpha.flac"),
        "Down should move selection through the newly added source before opening"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_opening_local_source_focuses_center_for_enter_playback() {
    let dir = test_dir("open-local-source-enter-playback");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - First.flac"), "").unwrap();
    std::fs::write(dir.join("02 - Second.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        config,
        None,
        None,
        PlaylistStore::with_dir(test_dir("open-local-source-enter-playback-playlists")),
        Box::new(player),
    );

    dispatch_key(&mut app, make_key(KeyCode::Enter));

    assert_eq!(app.focused_window, FocusedWindow::Center);

    dispatch_key(&mut app, make_key(KeyCode::Enter));
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "01 - First.flac");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_editing_local_source_updates_left_panel_immediately() {
    let old_dir = test_dir("edit-local-source-old");
    let new_dir = test_dir("edit-local-source-new");
    std::fs::create_dir_all(&old_dir).unwrap();
    std::fs::create_dir_all(&new_dir).unwrap();
    std::fs::write(old_dir.join("01 - Old.flac"), "").unwrap();
    std::fs::write(new_dir.join("01 - New.flac"), "").unwrap();

    let mut config = default_config();
    config.local.sources.push(LocalSource {
        name: "Old Library".to_string(),
        path: old_dir.clone(),
    });
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('e')));
    for _ in 0..64 {
        app.delegate_key_to_panel(make_key(KeyCode::Backspace));
    }
    for c in "New Library".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for _ in 0..256 {
        app.delegate_key_to_panel(make_key(KeyCode::Backspace));
    }
    for c in new_dir.to_string_lossy().chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Updated"),
        "editing a local source should show settings feedback"
    );

    app.delegate_key_to_panel(make_key(KeyCode::Esc));
    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("New Library"),
        "edited source name should appear in the Local tab without restarting"
    );
    assert!(
        text.contains("01 - New.flac"),
        "edited source path should be selectable immediately"
    );
    assert!(
        !text.contains("01 - Old.flac"),
        "the old source path should no longer populate the Local tab"
    );

    let _ = std::fs::remove_dir_all(old_dir);
    let _ = std::fs::remove_dir_all(new_dir);
}

#[test]
fn test_reordering_local_sources_updates_left_panel_immediately() {
    let first_dir = test_dir("reorder-local-source-first");
    let second_dir = test_dir("reorder-local-source-second");
    std::fs::create_dir_all(&first_dir).unwrap();
    std::fs::create_dir_all(&second_dir).unwrap();
    std::fs::write(first_dir.join("01 - First.flac"), "").unwrap();
    std::fs::write(second_dir.join("01 - Second.flac"), "").unwrap();

    let mut config = default_config();
    config.local.sources.push(LocalSource {
        name: "First Library".to_string(),
        path: first_dir.clone(),
    });
    config.local.sources.push(LocalSource {
        name: "Second Library".to_string(),
        path: second_dir.clone(),
    });
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('J')));
    app.tick();

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Moved First Library down"),
        "reordering a local source should show settings feedback"
    );

    app.delegate_key_to_panel(make_key(KeyCode::Esc));
    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Second Library"),
        "moved source order should refresh in the Local tab without restarting"
    );
    assert!(
        text.contains("01 - Second.flac"),
        "the new first source should be selectable immediately"
    );
    assert!(
        !text.contains("01 - First.flac"),
        "the old first source should no longer be selected after reordering"
    );

    let _ = std::fs::remove_dir_all(first_dir);
    let _ = std::fs::remove_dir_all(second_dir);
}

#[test]
fn test_renaming_open_local_source_updates_center_title() {
    let dir = test_dir("rename-open-local-source");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Track.flac"), "").unwrap();

    let mut config = default_config();
    config.local.sources.push(LocalSource {
        name: "Old Library".to_string(),
        path: dir.clone(),
    });
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::SelectAlbum);
    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('e')));
    for _ in 0..64 {
        app.delegate_key_to_panel(make_key(KeyCode::Backspace));
    }
    for c in "New Library".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();
    app.delegate_key_to_panel(make_key(KeyCode::Esc));

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("New Library"),
        "renamed source should refresh in the open center view"
    );
    assert!(
        text.contains("01 - Track.flac"),
        "renaming a source should keep its tracks visible"
    );
    assert!(
        !text.contains("Old Library"),
        "the open center view should not keep the stale source title"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_selecting_empty_left_tab_shows_contextual_feedback() {
    let mut app = make_app(None, None);

    app.execute(Action::SelectAlbum);
    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::SelectAlbum);

    let backend = TestBackend::new(320, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("No local albums"),
        "empty Local selection should explain how to populate the library"
    );
    assert!(
        text.contains("No playlists yet"),
        "empty Playlists selection should explain how to create a playlist"
    );
    assert!(
        text.contains("Use / to search Qobuz"),
        "empty streaming tabs should point users to search"
    );
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
fn test_streaming_album_search_no_results_shows_guidance() {
    let mock = MockStreamingService::new_authenticated("Qobuz", Vec::new());
    let mut app = make_app(Some(Box::new(mock)), None);

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "unknown".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Albums (0)"));
    assert!(
        text.contains("No matching albums"),
        "zero-result album search should explain that nothing matched"
    );
    assert!(
        text.contains("Press / to search again"),
        "zero-result album search should show the retry shortcut"
    );
}

#[test]
fn test_streaming_artist_search_no_results_shows_guidance() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let mut app = make_app(Some(Box::new(mock)), None);

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in "unknown".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Artists (0)"));
    assert!(
        text.contains("No matching artists"),
        "zero-result artist search should explain that nothing matched"
    );
    assert!(
        text.contains("Press / to search again"),
        "zero-result artist search should show the retry shortcut"
    );
}

#[test]
fn test_qobuz_search_without_credentials_shows_config_hint() {
    let mut app = make_app(None, None);

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "test".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Configure Qobuz in Settings"),
        "Qobuz search without credentials should show a configuration hint"
    );
    assert!(
        text.contains("Login Required"),
        "Qobuz search without credentials should show the login popup"
    );
    assert!(
        text.contains("Enter/Esc"),
        "login popup should show its dismiss keys"
    );
    assert!(
        !text.contains("Waiting for previous request cleanup"),
        "Unavailable Qobuz search should not look like a stuck request"
    );

    dispatch_key(&mut app, make_key(KeyCode::Esc));
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("Login Required"),
        "Esc should dismiss the login popup"
    );
    assert!(
        text.contains("Configure Qobuz in Settings"),
        "dismissing the popup should keep the underlying status hint"
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
fn test_local_search_filters_album_songs_by_file_path() {
    let mut app = make_app(None, None);

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "Tagged Title".to_string(),
                artist: "Tagged Artist".to_string(),
                album_name: "Tagged Album".to_string(),
                path: PathBuf::from("/music/rips/live-session-1999/01 - Untagged Demo.flac"),
                ..Default::default()
            },
            Song {
                title: "Other Title".to_string(),
                artist: "Other Artist".to_string(),
                album_name: "Other Album".to_string(),
                path: PathBuf::from("/music/rips/studio/02 - Other.flac"),
                ..Default::default()
            },
        ],
    );

    app.execute(Action::OpenSearch);
    for c in "untagged".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Tagged Artist - Tagged Title"),
        "local search should match filenames and paths when displayed metadata does not match"
    );
    assert!(
        !text.contains("Other Artist - Other Title"),
        "local search should keep non-matching paths filtered out"
    );
    assert!(text.contains("Search Results (1)"));
}

#[test]
fn test_local_search_scans_configured_sources_without_open_album() {
    let dir = test_dir("local-library-search");
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    std::fs::write(dir.join("01 - Love Will Tear Us Apart.flac"), "").unwrap();
    std::fs::write(dir.join("nested").join("02 - Disorder.flac"), "").unwrap();

    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);

    assert_eq!(app.left_panel.active_tab_name(), "Local");
    app.execute(Action::OpenSearch);
    assert_eq!(app.focused_window, FocusedWindow::Center);

    for c in "love".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Love Will Tear Us Apart"));
    assert!(
        !text.contains("Disorder"),
        "local library search should filter across configured sources"
    );
    assert!(text.contains("Search Results (1)"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_local_search_input_shows_filter_context() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Love Will Tear Us Apart".to_string(),
            path: PathBuf::from("/music/album/01.flac"),
            ..Default::default()
        }],
    );

    app.execute(Action::OpenSearch);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Filter Songs"),
        "local album search should be labeled as a song filter"
    );
    assert!(
        !text.contains("Search Albums"),
        "local filter should not show streaming search mode copy"
    );
}

#[test]
fn test_local_search_input_lists_loaded_songs() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "Love Will Tear Us Apart".to_string(),
                path: PathBuf::from("/music/album/01.flac"),
                ..Default::default()
            },
            Song {
                title: "Disorder".to_string(),
                path: PathBuf::from("/music/album/02.flac"),
                ..Default::default()
            },
        ],
    );

    app.execute(Action::OpenSearch);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Love Will Tear Us Apart"),
        "local filter should keep the loaded songs visible while entering a query"
    );
    assert!(
        text.contains("Disorder"),
        "local filter should list the rest of the loaded album while entering a query"
    );
}

#[test]
fn test_local_search_filters_loaded_songs_while_typing() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "Love Will Tear Us Apart".to_string(),
                path: PathBuf::from("/music/album/01.flac"),
                ..Default::default()
            },
            Song {
                title: "Disorder".to_string(),
                path: PathBuf::from("/music/album/02.flac"),
                ..Default::default()
            },
        ],
    );

    app.execute(Action::OpenSearch);
    for c in "love".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Love Will Tear Us Apart"));
    assert!(
        !text.contains("Disorder"),
        "local filter should narrow the visible songs before Enter is pressed"
    );
    assert!(
        text.contains("Songs (1)"),
        "the live local filter should update the in-input result count"
    );
}

#[test]
fn test_local_search_filters_album_songs_by_artist() {
    let mut app = make_app(None, None);

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "01 - Disorder.flac".to_string(),
                artist: "Joy Division".to_string(),
                path: PathBuf::from("/music/album/01.flac"),
                ..Default::default()
            },
            Song {
                title: "02 - Transmission.flac".to_string(),
                artist: "New Order".to_string(),
                path: PathBuf::from("/music/album/02.flac"),
                ..Default::default()
            },
        ],
    );

    app.execute(Action::OpenSearch);
    for c in "joy".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Disorder"), "Artist match should appear");
    assert!(
        !text.contains("Transmission"),
        "Non-matching artist should be filtered out"
    );
    assert!(
        text.contains("Search Results (1)"),
        "Should show 1 artist-filtered result"
    );
}

#[test]
fn test_local_search_filters_album_songs_by_album_name() {
    let mut app = make_app(None, None);

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "01 - Age of Consent.flac".to_string(),
                artist: "New Order".to_string(),
                album_name: "Power Corruption and Lies".to_string(),
                path: PathBuf::from("/music/album/01.flac"),
                ..Default::default()
            },
            Song {
                title: "02 - Ceremony.flac".to_string(),
                artist: "New Order".to_string(),
                album_name: "Substance".to_string(),
                path: PathBuf::from("/music/album/02.flac"),
                ..Default::default()
            },
        ],
    );

    app.execute(Action::OpenSearch);
    for c in "substance".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Ceremony"), "Album match should appear");
    assert!(
        !text.contains("Age of Consent"),
        "Non-matching album should be filtered out"
    );
    assert!(
        text.contains("Search Results (1)"),
        "Should show 1 album-filtered result"
    );
}

#[test]
fn test_local_search_can_reopen_after_zero_results() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Love Will Tear Us Apart".to_string(),
            path: PathBuf::from("/music/album/01.flac"),
            ..Default::default()
        }],
    );

    app.execute(Action::OpenSearch);
    for c in "missing".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    assert!(app.center_panel.get_songs().is_empty());

    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Search Results (0)"));
    assert!(
        text.contains("No matching songs"),
        "zero-result local filter should explain that nothing matched"
    );

    app.execute(Action::OpenSearch);

    assert!(
        app.center_panel.is_search_input_active(),
        "local filter should reopen even when the previous filter had zero visible songs"
    );
}

#[test]
fn test_blank_local_search_restores_all_songs() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "Love Will Tear Us Apart".to_string(),
                path: PathBuf::from("/music/album/01.flac"),
                ..Default::default()
            },
            Song {
                title: "Disorder".to_string(),
                path: PathBuf::from("/music/album/02.flac"),
                ..Default::default()
            },
        ],
    );

    app.execute(Action::OpenSearch);
    for c in "missing".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();
    assert!(app.center_panel.get_songs().is_empty());

    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    assert!(app.center_panel.is_showing_search_results());

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Love Will Tear Us Apart"));
    assert!(text.contains("Disorder"));
    assert!(text.contains("Search Results (2)"));
}

#[test]
fn test_add_to_playlist_uses_selected_song() {
    let dir = test_dir("selected-playlist-song");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Selected Song".to_string(),
                artist: "Artist".to_string(),
                path: PathBuf::from("/music/selected.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    app.execute(Action::AddToPlaylist);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Selected Song");
    assert_eq!(
        playlists[0].tracks[0].path.as_deref(),
        Some("/music/selected.flac")
    );

    let backend = TestBackend::new(520, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added Artist - Selected Song to 'Mix'"),
        "adding a selected song to a playlist should confirm the track and playlist"
    );
    assert!(
        !text.contains("song(s)"),
        "add-to-playlist feedback should use normal wording"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_right_panel_add_current_track_to_playlist() {
    let dir = test_dir("right-panel-current-track-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Current Song".to_string(),
            artist: "Current Artist".to_string(),
            path: PathBuf::from("/music/current.flac"),
            ..Default::default()
        }],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.focused_window = FocusedWindow::Right;

    dispatch_key(&mut app, make_key(KeyCode::Char('A')));
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Current Song");
    assert_eq!(
        playlists[0].tracks[0].path.as_deref(),
        Some("/music/current.flac")
    );

    let backend = TestBackend::new(280, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added Current Artist - Current Song to 'Mix'"),
        "adding the current track should use the normal playlist feedback"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_right_panel_add_current_track_without_playback_shows_feedback() {
    let mut app = make_app(None, None);
    app.focused_window = FocusedWindow::Right;

    dispatch_key(&mut app, make_key(KeyCode::Char('A')));
    app.tick();

    let backend = TestBackend::new(180, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("No current track"),
        "adding the current track without playback should explain what is missing"
    );
}

#[test]
fn test_add_open_collection_to_existing_playlist_uses_all_tracks() {
    let dir = test_dir("open-collection-existing-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    dispatch_key(&mut app, make_key(KeyCode::Char('C')));
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(
        playlists[0]
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["First Song", "Second Song"],
        "center C should add the full open collection, not only the selected song"
    );

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added 2 tracks to 'Mix'"),
        "collection-to-playlist feedback should use multi-track wording"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_add_open_collection_to_new_playlist_when_none_exist() {
    let dir = test_dir("open-collection-new-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;

    dispatch_key(&mut app, make_key(KeyCode::Char('C')));
    for c in "Album Save".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Album Save");
    assert_eq!(
        playlists[0]
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["First Song", "Second Song"]
    );

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added 2 tracks to 'Album Save'"),
        "collection-to-playlist should create and populate a new playlist"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_add_open_collection_without_collection_shows_feedback() {
    let mut app = make_app(None, None);
    app.focused_window = FocusedWindow::Center;

    dispatch_key(&mut app, make_key(KeyCode::Char('C')));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Open an album or playlist"),
        "center C without an open collection should explain the required context"
    );
}

#[test]
fn test_enqueue_open_collection_without_collection_shows_feedback() {
    let mut app = make_app(None, None);
    app.focused_window = FocusedWindow::Center;

    dispatch_key(&mut app, make_key(KeyCode::Char('E')));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Open an album or playlist"),
        "center E without an open collection should explain the required context"
    );
}

#[test]
fn test_add_to_playlist_that_disappeared_shows_feedback() {
    let dir = test_dir("add-to-stale-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Target".to_string()).unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Song To Add".to_string(),
            artist: "Artist".to_string(),
            path: PathBuf::from("/music/song.flac"),
            ..Default::default()
        }],
    );
    app.focused_window = FocusedWindow::Center;

    app.execute(Action::AddToPlaylist);
    store.delete_at(0).unwrap();
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Playlist no longer exists"),
        "stale playlist add should explain that the backing file disappeared"
    );
    assert!(
        store.load_all().is_empty(),
        "deleted playlist should not be recreated by a stale picker selection"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_add_to_playlist_without_existing_playlists_creates_and_adds() {
    let dir = test_dir("add-to-new-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "First Pick".to_string(),
            artist: "Artist".to_string(),
            path: PathBuf::from("/music/first-pick.flac"),
            ..Default::default()
        }],
    );
    app.focused_window = FocusedWindow::Center;

    app.execute(Action::AddToPlaylist);
    for c in "Road Mix".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Road Mix");
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "First Pick");
    assert_eq!(
        playlists[0].tracks[0].path.as_deref(),
        Some("/music/first-pick.flac")
    );

    switch_to_tab(&mut app, "Playlists");

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Road Mix (1 track)"),
        "new playlist should appear in the Playlists tab with the added song"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_create_duplicate_playlist_does_not_overwrite_existing_tracks() {
    let dir = test_dir("duplicate-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Keep Me".to_string(),
                artist: "Artist".to_string(),
                path: PathBuf::from("/music/keep.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::CreatePlaylist);
    for c in "Mix".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("New Playlist Name"),
        "duplicate playlist feedback should keep the create dialog open"
    );
    assert!(
        text.contains("Playlist already exists"),
        "duplicate playlist feedback should be visible inline"
    );

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Keep Me");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_rename_playlist_preserves_tracks_and_open_view() {
    let dir = test_dir("rename-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Road".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Night Drive".to_string(),
                artist: "Driver".to_string(),
                path: PathBuf::from("/music/night-drive.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.execute(Action::RenamePlaylist);
    for _ in "Road".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Backspace));
    }
    for c in "Sleep Songs".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Char(c)));
    }
    dispatch_key(&mut app, make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Sleep Songs");
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Night Drive");
    assert!(
        !dir.join("Road.toml").exists(),
        "renaming should remove the old playlist file"
    );

    let backend = TestBackend::new(240, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Renamed playlist 'Road' to 'Sleep Songs'"),
        "rename should show feedback with old and new names"
    );
    assert!(
        text.contains("Sleep Songs (1 track)"),
        "renamed playlist should refresh in the left panel and open center title"
    );
    assert!(
        text.contains("Driver - Night Drive"),
        "open playlist view should keep showing renamed playlist tracks"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_duplicate_playlist_rename_keeps_dialog_open() {
    let dir = test_dir("rename-duplicate-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Road".to_string()).unwrap();
    store.create("Sleep".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Road Song".to_string(),
                path: PathBuf::from("/music/road.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::RenamePlaylist);
    for _ in "Road".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Backspace));
    }
    for c in "sleep".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Char(c)));
    }
    dispatch_key(&mut app, make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Rename Playlist"),
        "duplicate rename feedback should keep the rename dialog open"
    );
    assert!(
        text.contains("Playlist already exists"),
        "duplicate rename feedback should be visible inline"
    );

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 2);
    assert!(playlists.iter().any(|playlist| playlist.name == "Road"));
    assert!(playlists.iter().any(|playlist| playlist.name == "Sleep"));
    let road = playlists
        .iter()
        .find(|playlist| playlist.name == "Road")
        .expect("original playlist should still exist");
    assert_eq!(road.tracks.len(), 1);
    assert_eq!(road.tracks[0].title, "Road Song");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_duplicate_playlist_preserves_tracks_with_default_name() {
    let dir = test_dir("duplicate-playlist-copy");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Road".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Night Drive".to_string(),
                artist: "Driver".to_string(),
                album_name: "Road Album".to_string(),
                path: PathBuf::from("/music/night-drive.flac"),
                duration_secs: Some(210.0),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::DuplicatePlaylist);
    dispatch_key(&mut app, make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 2);
    let original = playlists
        .iter()
        .find(|playlist| playlist.name == "Road")
        .expect("original playlist should remain");
    let copy = playlists
        .iter()
        .find(|playlist| playlist.name == "Road Copy")
        .expect("copied playlist should be created");
    assert_eq!(original.tracks.len(), 1);
    assert_eq!(copy.tracks.len(), 1);
    assert_eq!(copy.tracks[0].title, "Night Drive");
    assert_eq!(copy.tracks[0].artist, "Driver");
    assert_eq!(copy.tracks[0].album_name, "Road Album");
    assert_eq!(copy.tracks[0].duration_secs, Some(210.0));

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Duplicated playlist 'Road' to 'Road Copy'"),
        "duplicate should show feedback with old and new names"
    );
    assert!(
        text.contains("Road Copy (1 track)"),
        "duplicated playlist should refresh into the Playlists tab"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_left_panel_filter_opens_filtered_playlist_selection() {
    let dir = test_dir("left-filter-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Road".to_string()).unwrap();
    store.create("Sleep".to_string()).unwrap();
    store
        .add_songs_to_index(
            1,
            &[Song {
                title: "Sleep Song".to_string(),
                artist: "Dreamer".to_string(),
                path: PathBuf::from("/music/sleep.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());
    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    switch_to_tab(&mut app, "Playlists");
    dispatch_key(&mut app, make_key(KeyCode::Char('f')));
    for c in "sleep".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Char(c)));
    }

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Filter List"),
        "left panel search should show the focused list filter"
    );
    assert!(
        text.contains("Sleep (1 track)"),
        "left panel filter should keep matching playlist rows visible"
    );
    assert!(
        !text.contains("Road (0 tracks)"),
        "left panel filter should hide non-matching playlist rows"
    );

    dispatch_key(&mut app, make_key(KeyCode::Enter));
    app.execute(Action::SelectAlbum);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Sleep (1 track)"),
        "opening a filtered row should use the matching playlist"
    );
    assert!(
        text.contains("Dreamer - Sleep Song"),
        "opening a filtered playlist should show that playlist's tracks"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_remove_selected_track_from_playlist() {
    let dir = test_dir("remove-playlist-track");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Remove Me".to_string(),
                    artist: "Artist".to_string(),
                    path: PathBuf::from("/music/remove.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Keep Me".to_string(),
                    artist: "Artist".to_string(),
                    path: PathBuf::from("/music/keep.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Keep Me");

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Removed Artist - Remove Me from 'Mix'"),
        "removing a playlist track should confirm the track and playlist"
    );
    assert!(
        !text.contains("Removed 'Remove Me' from 'Mix'"),
        "playlist removal feedback should include artist context when available"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_remove_track_on_local_album_shows_playlist_hint() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Local Track".to_string(),
            path: PathBuf::from("/music/album/local.flac"),
            ..Default::default()
        }],
    );

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Open a playlist"),
        "pressing d on a local album should explain that removal only applies to playlists"
    );
}

#[test]
fn test_move_selected_playlist_track_down_persists_order_and_selection() {
    let dir = test_dir("move-playlist-track");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "First".to_string(),
                    artist: "Artist".to_string(),
                    path: PathBuf::from("/music/first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second".to_string(),
                    artist: "Artist".to_string(),
                    path: PathBuf::from("/music/second.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Third".to_string(),
                    artist: "Artist".to_string(),
                    path: PathBuf::from("/music/third.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('J')));
    app.tick();

    let loaded = store.load_all();
    assert_eq!(
        loaded[0]
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["First", "Third", "Second"]
    );
    assert_eq!(
        app.center_panel.selected_songs_for_playlist()[0].title,
        "Second",
        "moved playlist track should stay selected after the view refreshes"
    );

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Moved Artist - Second down in 'Mix'"),
        "playlist move feedback should name the moved track and direction"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_playlist_track_move_boundary_shows_feedback() {
    let dir = test_dir("move-playlist-track-boundary");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mix".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "First".to_string(),
                    path: PathBuf::from("/music/first.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Second".to_string(),
                    path: PathBuf::from("/music/second.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('K')));
    app.tick();

    let loaded = store.load_all();
    assert_eq!(
        loaded[0]
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["First", "Second"]
    );

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Cannot move playlist track further"),
        "boundary playlist moves should explain why nothing changed"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_move_track_on_local_album_shows_playlist_hint() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Local Track".to_string(),
            path: PathBuf::from("/music/album/local.flac"),
            ..Default::default()
        }],
    );

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('J')));
    app.tick();

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Open a playlist to reorder"),
        "pressing J on a local album should explain that reordering only applies to playlists"
    );
}

#[test]
fn test_open_playlist_removal_survives_playlist_reordering() {
    let dir = test_dir("playlist-stable-path");
    let store = PlaylistStore::with_dir(dir.clone());
    for (name, title) in [
        ("Alpha", "Alpha Keep"),
        ("Mix", "Mix Remove"),
        ("Zed", "Zed Keep"),
    ] {
        store.create(name.to_string()).unwrap();
        store
            .add_songs_to_index(
                store
                    .load_all()
                    .iter()
                    .position(|playlist| playlist.name == name)
                    .unwrap(),
                &[Song {
                    title: title.to_string(),
                    artist: "Artist".to_string(),
                    path: PathBuf::from(format!("/music/{title}.flac")),
                    ..Default::default()
                }],
            )
            .unwrap();
    }
    let mix_index = store
        .load_all()
        .iter()
        .position(|playlist| playlist.name == "Mix")
        .unwrap();
    store
        .add_songs_to_index(
            mix_index,
            &[Song {
                title: "Mix Keep".to_string(),
                artist: "Artist".to_string(),
                path: PathBuf::from("/music/mix-keep.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.left_panel.handle_events(make_key(KeyCode::Char('j')));
    app.execute(Action::SelectAlbum);

    app.left_panel.handle_events(make_key(KeyCode::Char('k')));
    app.execute(Action::DeletePlaylist);

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let playlists = store.load_all();
    let mix = playlists
        .iter()
        .find(|playlist| playlist.name == "Mix")
        .unwrap();
    let zed = playlists
        .iter()
        .find(|playlist| playlist.name == "Zed")
        .unwrap();
    assert_eq!(
        mix.tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Mix Keep"]
    );
    assert_eq!(
        zed.tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Zed Keep"]
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_opened_playlist_uses_playlist_name_as_title() {
    let dir = test_dir("playlist-title");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Readable Mix".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Track".to_string(),
                path: PathBuf::from("/music/track.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Readable Mix"));
    assert!(
        !text.contains("playlist:0"),
        "opened playlist should not expose internal path encoding"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_opened_playlist_tracks_show_artist_context() {
    let dir = test_dir("playlist-artist-context");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mixed Artists".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Ceremony".to_string(),
                    artist: "New Order".to_string(),
                    path: PathBuf::from("/music/ceremony.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Age of Consent".to_string(),
                    artist: "New Order".to_string(),
                    path: PathBuf::from("/music/age-of-consent.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("New Order - Ceremony"),
        "opened playlists should render artist context for each track"
    );
    assert!(
        text.contains("New Order - Age of Consent"),
        "opened playlists should keep artist context across multiple tracks"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_deleting_open_playlist_clears_center_view() {
    let dir = test_dir("delete-open-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Disposable".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Delete Me".to_string(),
                path: PathBuf::from("/music/delete-me.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Delete Me"));

    app.execute(Action::DeletePlaylist);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("Delete Me"),
        "deleted playlist tracks should not remain visible in the center panel"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_delete_playlist_that_disappeared_refreshes_view_with_feedback() {
    let dir = test_dir("delete-stale-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Ghost".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Ghost Track".to_string(),
                path: PathBuf::from("/music/ghost.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    store.delete_at(0).unwrap();

    app.execute(Action::DeletePlaylist);
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Playlist no longer exists"),
        "stale playlist deletion should explain that the backing file disappeared"
    );
    assert!(
        !text.contains("Ghost Track"),
        "stale opened playlist tracks should be cleared from the center panel"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_remove_track_from_playlist_that_disappeared_clears_view_with_feedback() {
    let dir = test_dir("remove-track-stale-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Ghost".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Ghost Track".to_string(),
                path: PathBuf::from("/music/ghost.flac"),
                ..Default::default()
            }],
        )
        .unwrap();
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    store.delete_at(0).unwrap();
    app.focused_window = FocusedWindow::Center;

    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Playlist no longer exists"),
        "stale playlist track removal should explain that the backing file disappeared"
    );
    assert!(
        !text.contains("Ghost Track"),
        "stale opened playlist tracks should be cleared from the center panel"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_delete_playlist_on_empty_tab_shows_feedback() {
    let dir = test_dir("delete-empty-playlists");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::DeletePlaylist);
    app.tick();

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("No playlist"),
        "deleting from an empty Playlists tab should tell the user nothing was selected"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_delete_playlist_outside_playlists_tab_shows_feedback() {
    let dir = test_dir("delete-outside-playlists");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    assert_eq!(app.left_panel.active_tab_name(), "Local");

    app.execute(Action::DeletePlaylist);
    app.tick();

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Switch to the Playlists"),
        "deleting outside the Playlists tab should explain where playlist deletion is available"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_refresh_library_loads_external_playlist_and_keeps_tab() {
    let dir = test_dir("refresh-external-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    switch_to_tab(&mut app, "Playlists");
    store.create("External Mix".to_string()).unwrap();

    app.execute(Action::RefreshLibrary);

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert_eq!(app.left_panel.active_tab_name(), "Playlists");
    assert!(
        text.contains("External Mix (0 tracks)"),
        "refresh should pick up playlists created outside the running app"
    );
    assert!(
        text.contains("Library refreshed"),
        "refresh should give visible feedback"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_refresh_library_updates_open_local_source_tracks() {
    let dir = test_dir("refresh-open-local-source");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Existing.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::SelectAlbum);
    std::fs::write(dir.join("02 - Added.flac"), "").unwrap();
    app.execute(Action::RefreshLibrary);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("01 - Existing.flac"),
        "refresh should keep existing tracks visible"
    );
    assert!(
        text.contains("02 - Added.flac"),
        "refresh should reload newly added tracks in the open local source"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_refresh_library_updates_open_discovered_local_album_tracks() {
    let dir = test_dir("refresh-open-discovered-local-album");
    let album_dir = dir.join("Album One");
    std::fs::create_dir_all(&album_dir).unwrap();
    std::fs::write(album_dir.join("01 - Existing.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::SelectAlbum);
    std::fs::write(album_dir.join("02 - Added.flac"), "").unwrap();
    app.execute(Action::RefreshLibrary);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Album One"),
        "refresh should preserve the discovered album title"
    );
    assert!(
        text.contains("01 - Existing.flac"),
        "refresh should keep existing discovered album tracks visible"
    );
    assert!(
        text.contains("02 - Added.flac"),
        "refresh should reload newly added tracks in the open discovered album folder"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_refresh_library_keeps_open_source_root_tracks_separate_from_child_albums() {
    let dir = test_dir("refresh-open-source-root-with-child-album");
    let child_dir = dir.join("Child Album");
    std::fs::create_dir_all(&child_dir).unwrap();
    std::fs::write(dir.join("00 - Root Track.flac"), "").unwrap();
    std::fs::write(child_dir.join("01 - Child Track.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.left_panel.handle_events(make_key(KeyCode::Down));
    app.execute(Action::SelectAlbum);
    std::fs::write(dir.join("02 - Added Root Track.flac"), "").unwrap();
    app.execute(Action::RefreshLibrary);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("00 - Root Track.flac"));
    assert!(text.contains("02 - Added Root Track.flac"));
    assert!(
        !text.contains("01 - Child Track.flac"),
        "refreshing a source-root entry should not reintroduce discovered child album tracks"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_refresh_library_clears_open_discovered_album_that_was_removed() {
    let dir = test_dir("refresh-removed-open-discovered-album");
    let album_dir = dir.join("Removed Album");
    std::fs::create_dir_all(&album_dir).unwrap();
    std::fs::write(album_dir.join("01 - Gone.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("01 - Gone.flac"));

    std::fs::remove_dir_all(&album_dir).unwrap();
    app.execute(Action::RefreshLibrary);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("01 - Gone.flac"),
        "refresh should clear tracks from an externally removed local album"
    );
    assert!(
        text.contains("Select an album or playlist"),
        "refresh should return the center panel to guidance when the open local album disappears"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_refresh_library_updates_open_local_library_search() {
    let dir = test_dir("refresh-open-local-library-search");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("01 - Existing.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();
    std::fs::write(dir.join("02 - Added.flac"), "").unwrap();
    app.execute(Action::RefreshLibrary);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("01 - Existing.flac"),
        "refresh should keep existing all-library search results visible"
    );
    assert!(
        text.contains("02 - Added.flac"),
        "refresh should reload newly added tracks in the open all-library search view"
    );
    assert!(
        text.contains("Search Results (2)"),
        "refresh should update the all-library search result count"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_removing_source_updates_open_local_library_search() {
    let removed_dir = test_dir("remove-source-local-library-search-removed");
    let kept_dir = test_dir("remove-source-local-library-search-kept");
    std::fs::create_dir_all(&removed_dir).unwrap();
    std::fs::create_dir_all(&kept_dir).unwrap();
    std::fs::write(removed_dir.join("01 - Removed.flac"), "").unwrap();
    std::fs::write(kept_dir.join("02 - Kept.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![
                LocalSource {
                    name: "Removed".to_string(),
                    path: removed_dir.clone(),
                },
                LocalSource {
                    name: "Kept".to_string(),
                    path: kept_dir.clone(),
                },
            ],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(200, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();
    app.execute(Action::ToggleSettings);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("01 - Removed.flac"),
        "open all-library search should drop tracks from removed local sources"
    );
    assert!(
        text.contains("02 - Kept.flac"),
        "open all-library search should keep tracks from remaining local sources"
    );
    assert!(
        text.contains("Search Results (1)"),
        "open all-library search count should refresh after source removal"
    );

    let _ = std::fs::remove_dir_all(removed_dir);
    let _ = std::fs::remove_dir_all(kept_dir);
}

#[test]
fn test_add_streaming_track_to_playlist_preserves_service_metadata() {
    let dir = test_dir("streaming-playlist-track");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Streams".to_string()).unwrap();
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let mut app = App::new_for_test_with_playlist_store(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "album".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    app.execute(Action::AddToPlaylist);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Track2");
    assert_eq!(playlists[0].tracks[0].path, None);
    assert_eq!(
        playlists[0].tracks[0].stream_service.as_deref(),
        Some("Qobuz")
    );
    assert_eq!(playlists[0].tracks[0].stream_track_id.as_deref(), Some("2"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_add_track_search_result_to_playlist_preserves_canonical_title() {
    let dir = test_dir("track-search-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Tracks".to_string()).unwrap();
    let mock =
        MockStreamingService::new_authenticated("Qobuz", mock_albums()).with_track_results(vec![
            StreamTrack {
                id: "track1".to_string(),
                title: "Ceremony".to_string(),
                artist: "New Order".to_string(),
                album: "Substance".to_string(),
            },
        ]);
    let mut app = App::new_for_test_with_playlist_store(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in "ceremony".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("New Order - Ceremony"),
        "Track search results should render artist context"
    );

    app.execute(Action::AddToPlaylist);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Ceremony");
    assert_eq!(playlists[0].tracks[0].artist, "New Order");
    assert_eq!(playlists[0].tracks[0].album_name, "Substance");
    assert_eq!(
        playlists[0].tracks[0].stream_service.as_deref(),
        Some("Qobuz")
    );
    assert_eq!(
        playlists[0].tracks[0].stream_track_id.as_deref(),
        Some("track1")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_play_track_search_result_preserves_stream_metadata() {
    let dir = test_dir("track-search-playback");
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums())
        .with_track_results(vec![StreamTrack {
            id: "track1".to_string(),
            title: "Ceremony".to_string(),
            artist: "New Order".to_string(),
            album: "Substance".to_string(),
        }])
        .with_stream_url("track1", "https://example.com/ceremony.flac");
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        PlaylistStore::with_dir(dir.clone()),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in "ceremony".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Ceremony");
    assert_eq!(played[0].artist, "New Order");
    assert_eq!(played[0].album_name, "Substance");
    assert_eq!(played[0].stream_service.as_deref(), Some("Qobuz"));
    assert_eq!(played[0].stream_track_id.as_deref(), Some("track1"));
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/ceremony.flac")
    );
    drop(played);

    let backend = TestBackend::new(320, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Playing New Order - Ceremony"),
        "resolved streaming playback should confirm the current track"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_enter_on_track_search_result_starts_playback() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums())
        .with_track_results(vec![StreamTrack {
            id: "track1".to_string(),
            title: "Ceremony".to_string(),
            artist: "New Order".to_string(),
            album: "Substance".to_string(),
        }])
        .with_stream_url("track1", "https://example.com/ceremony.flac");
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        PlaylistStore::with_dir(test_dir("enter-track-search-playback")),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in "ceremony".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Ceremony");
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/ceremony.flac")
    );
}

#[test]
fn test_play_streaming_album_preserves_track_metadata() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums())
        .with_stream_url("1", "https://example.com/track1.flac")
        .with_stream_url("2", "https://example.com/track2.flac");
    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        PlaylistStore::with_dir(test_dir("streaming-album-playback")),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "album".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Track2");
    assert_eq!(played[0].artist, "Artist1");
    assert_eq!(played[0].album_name, "Album1");
    assert_eq!(played[0].stream_service.as_deref(), Some("Qobuz"));
    assert_eq!(played[0].stream_track_id.as_deref(), Some("2"));
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/track2.flac")
    );

    assert_eq!(enqueued.len(), 1);
    assert_eq!(enqueued[0].title, "Track1");
    assert_eq!(enqueued[0].artist, "Artist1");
    assert_eq!(enqueued[0].album_name, "Album1");
    assert_eq!(enqueued[0].stream_service.as_deref(), Some("Qobuz"));
    assert_eq!(enqueued[0].stream_track_id.as_deref(), Some("1"));
    assert_eq!(
        enqueued[0].url.as_deref(),
        Some("https://example.com/track1.flac")
    );
}

#[test]
fn test_play_streaming_album_reports_unresolved_tracks_with_plural_feedback() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums())
        .with_album_tracks(vec![
            StreamTrack {
                id: "1".to_string(),
                title: "Track1".to_string(),
                artist: "Artist1".to_string(),
                album: "Album1".to_string(),
            },
            StreamTrack {
                id: "2".to_string(),
                title: "Missing Two".to_string(),
                artist: "Artist1".to_string(),
                album: "Album1".to_string(),
            },
            StreamTrack {
                id: "3".to_string(),
                title: "Missing Three".to_string(),
                artist: "Artist1".to_string(),
                album: "Album1".to_string(),
            },
        ])
        .with_stream_url("1", "https://example.com/track1.flac")
        .with_missing_stream_url("2")
        .with_missing_stream_url("3");
    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        PlaylistStore::with_dir(test_dir("streaming-album-missing-tracks")),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "album".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Track1");
    assert!(enqueued.is_empty());
    drop(enqueued);
    drop(played);

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("2 tracks could not be resolved"));
    assert!(
        !text.contains("track(s)"),
        "unresolved-track feedback should use normal plural wording"
    );
}

#[test]
fn test_play_streaming_playlist_track_resolves_before_player() {
    let dir = test_dir("play-streaming-playlist-track");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Streams".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Saved Stream".to_string(),
                artist: "Artist".to_string(),
                album_name: "Album".to_string(),
                stream_service: Some("Qobuz".to_string()),
                stream_track_id: Some("2".to_string()),
                ..Default::default()
            }],
        )
        .unwrap();

    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Saved Stream");
    assert_eq!(played[0].artist, "Artist");
    assert_eq!(played[0].album_name, "Album");
    assert_eq!(played[0].stream_service.as_deref(), Some("Qobuz"));
    assert_eq!(played[0].stream_track_id.as_deref(), Some("2"));
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/stream.flac")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_play_streaming_playlist_resolves_remaining_tracks() {
    let dir = test_dir("play-streaming-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Streams".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Saved One".to_string(),
                    artist: "Artist".to_string(),
                    album_name: "Album".to_string(),
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("1".to_string()),
                    ..Default::default()
                },
                Song {
                    title: "Saved Two".to_string(),
                    artist: "Artist".to_string(),
                    album_name: "Album".to_string(),
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("2".to_string()),
                    ..Default::default()
                },
            ],
        )
        .unwrap();

    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(enqueued.len(), 1);
    assert_eq!(played[0].title, "Saved Two");
    assert_eq!(enqueued[0].title, "Saved One");
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/stream.flac")
    );
    assert_eq!(
        enqueued[0].url.as_deref(),
        Some("https://example.com/stream.flac")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_play_mixed_playlist_resolves_streams_and_keeps_local_tracks() {
    let dir = test_dir("play-mixed-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mixed".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Local Track".to_string(),
                    artist: "Local Artist".to_string(),
                    path: PathBuf::from("/music/local.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Saved Stream".to_string(),
                    artist: "Stream Artist".to_string(),
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("2".to_string()),
                    ..Default::default()
                },
            ],
        )
        .unwrap();

    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(enqueued.len(), 1);
    assert_eq!(played[0].title, "Saved Stream");
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/stream.flac")
    );
    assert_eq!(enqueued[0].title, "Local Track");
    assert_eq!(enqueued[0].path, PathBuf::from("/music/local.flac"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_play_mixed_playlist_from_local_track_enqueues_streams() {
    let dir = test_dir("play-mixed-playlist-from-local");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mixed".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Local Track".to_string(),
                    artist: "Local Artist".to_string(),
                    path: PathBuf::from("/music/local.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Saved Stream".to_string(),
                    artist: "Stream Artist".to_string(),
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("2".to_string()),
                    ..Default::default()
                },
            ],
        )
        .unwrap();

    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Local Track");
    assert_eq!(played[0].path, PathBuf::from("/music/local.flac"));
    assert_eq!(enqueued.len(), 1);
    assert_eq!(enqueued[0].title, "Saved Stream");
    assert_eq!(enqueued[0].artist, "Stream Artist");
    assert_eq!(
        enqueued[0].url.as_deref(),
        Some("https://example.com/stream.flac")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_play_mixed_playlist_skips_unknown_stream_service_and_continues() {
    let dir = test_dir("play-mixed-playlist-unknown-service");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mixed Unknown".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Local Start".to_string(),
                    artist: "Local Artist".to_string(),
                    path: PathBuf::from("/music/start.flac"),
                    ..Default::default()
                },
                Song {
                    title: "Saved Stream".to_string(),
                    artist: "Stream Artist".to_string(),
                    stream_service: Some("Unknown".to_string()),
                    stream_track_id: Some("bad-1".to_string()),
                    ..Default::default()
                },
                Song {
                    title: "Local Tail".to_string(),
                    artist: "Local Artist".to_string(),
                    path: PathBuf::from("/music/tail.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();

    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Local Start");
    assert_eq!(enqueued.len(), 1);
    assert_eq!(enqueued[0].title, "Local Tail");
    drop(enqueued);
    drop(played);

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Unknown streaming service 'Unknown'"));
    assert!(text.contains("1 track could not be resolved"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_play_multi_service_playlist_resolves_each_service_and_keeps_local_tracks() {
    let dir = test_dir("play-multi-service-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Mixed Services".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[
                Song {
                    title: "Qobuz Track".to_string(),
                    artist: "Qobuz Artist".to_string(),
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("q1".to_string()),
                    ..Default::default()
                },
                Song {
                    title: "Tidal Track".to_string(),
                    artist: "Tidal Artist".to_string(),
                    stream_service: Some("Tidal".to_string()),
                    stream_track_id: Some("t1".to_string()),
                    ..Default::default()
                },
                Song {
                    title: "Local Track".to_string(),
                    artist: "Local Artist".to_string(),
                    path: PathBuf::from("/music/local.flac"),
                    ..Default::default()
                },
            ],
        )
        .unwrap();

    let qobuz = MockStreamingService::new_authenticated("Qobuz", mock_albums())
        .with_stream_url("q1", "https://example.com/qobuz.flac");
    let tidal = MockStreamingService::new_authenticated("Tidal", mock_albums())
        .with_stream_url("t1", "https://example.com/tidal.flac");
    let (player, played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(qobuz)),
        Some(Box::new(tidal)),
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();
    app.tick();

    let played = played.lock().unwrap();
    let enqueued = enqueued.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Tidal Track");
    assert_eq!(played[0].artist, "Tidal Artist");
    assert_eq!(played[0].stream_service.as_deref(), Some("Tidal"));
    assert_eq!(
        played[0].url.as_deref(),
        Some("https://example.com/tidal.flac")
    );

    assert_eq!(enqueued.len(), 2);
    assert_eq!(enqueued[0].title, "Local Track");
    assert_eq!(enqueued[0].path, PathBuf::from("/music/local.flac"));
    assert_eq!(enqueued[1].title, "Qobuz Track");
    assert_eq!(enqueued[1].artist, "Qobuz Artist");
    assert_eq!(enqueued[1].stream_service.as_deref(), Some("Qobuz"));
    assert_eq!(
        enqueued[1].url.as_deref(),
        Some("https://example.com/qobuz.flac")
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_enqueue_selected_uses_selected_song() {
    let (player, _played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("enqueue-selected")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Queued Song".to_string(),
                path: PathBuf::from("/music/queued.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    app.execute(Action::EnqueueSelected);
    app.tick();

    let enqueued = enqueued.lock().unwrap();
    assert_eq!(enqueued.len(), 1);
    assert_eq!(enqueued[0].title, "Queued Song");
    drop(enqueued);

    let backend = TestBackend::new(320, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Queued Queued Song"),
        "enqueueing a selected local song should confirm which track was queued"
    );
}

#[test]
fn test_center_collection_queue_enqueues_open_local_album() {
    let (player, _played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("center-queue-local-album")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;

    app.execute(Action::EnqueueOpenCollection);
    app.tick();

    let enqueued = enqueued.lock().unwrap();
    assert_eq!(enqueued.len(), 2);
    assert_eq!(enqueued[0].title, "First Song");
    assert_eq!(enqueued[1].title, "Second Song");
    drop(enqueued);

    let backend = TestBackend::new(240, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Queued 2 tracks"),
        "queueing an open local album should show collection feedback"
    );
}

#[test]
fn test_center_collection_queue_resolves_streaming_album_tracks() {
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums())
        .with_stream_url("1", "https://example.com/track1.flac")
        .with_stream_url("2", "https://example.com/track2.flac");
    let (player, _played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        PlaylistStore::with_dir(test_dir("center-queue-streaming-album")),
        Box::new(player),
    );

    app.center_panel.set_album_tracks(
        "Artist1 - Album1".to_string(),
        vec![
            Song {
                title: "Track1".to_string(),
                artist: "Artist1".to_string(),
                album_name: "Album1".to_string(),
                stream_service: Some("Qobuz".to_string()),
                stream_track_id: Some("1".to_string()),
                ..Default::default()
            },
            Song {
                title: "Track2".to_string(),
                artist: "Artist1".to_string(),
                album_name: "Album1".to_string(),
                stream_service: Some("Qobuz".to_string()),
                stream_track_id: Some("2".to_string()),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;

    app.execute(Action::EnqueueOpenCollection);
    for _ in 0..4 {
        app.tick();
    }

    let enqueued = enqueued.lock().unwrap();
    assert_eq!(enqueued.len(), 2);
    assert_eq!(enqueued[0].title, "Track1");
    assert_eq!(
        enqueued[0].url.as_deref(),
        Some("https://example.com/track1.flac")
    );
    assert_eq!(enqueued[1].title, "Track2");
    assert_eq!(
        enqueued[1].url.as_deref(),
        Some("https://example.com/track2.flac")
    );
}

#[test]
fn test_play_selected_without_song_shows_feedback() {
    let mut app = make_app(None, None);

    app.execute(Action::PlaySelected);
    app.tick();

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Select a song"),
        "playing without a selected song should tell the user what is missing"
    );
}

#[test]
fn test_play_selected_song_shows_feedback() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("play-selected-feedback")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    app.execute(Action::PlaySelected);
    app.tick();

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Playing Second Artist - Second Song"),
        "starting playback should confirm the selected track"
    );
}

#[test]
fn test_enter_on_open_album_song_starts_playback() {
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("enter-open-album-playback")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Second Song");
    assert_eq!(played[0].artist, "Second Artist");
}

#[test]
fn test_open_album_marks_current_track() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("current-track-marker")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.tick();

    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains(">  1. First Song"),
        "open albums should mark the currently playing track"
    );
    assert!(
        !text.contains(">  2. Second Song"),
        "only the matching currently playing track should be marked"
    );
}

#[test]
fn test_enqueue_selected_without_song_shows_feedback() {
    let mut app = make_app(None, None);

    app.execute(Action::EnqueueSelected);
    app.tick();

    let backend = TestBackend::new(240, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Select a song"),
        "enqueueing without a selected song should tell the user what is missing"
    );
}

#[test]
fn test_pause_without_playback_shows_feedback() {
    let mut app = make_app(None, None);

    app.execute(Action::TogglePause);
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Nothing playing"),
        "pause without playback should explain that there is no active track"
    );
}

#[test]
fn test_seek_without_playback_shows_feedback() {
    let mut app = make_app(None, None);

    app.execute(Action::SeekForward(5.0));
    app.execute(Action::SeekBackward(5.0));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Nothing playing"),
        "seek without playback should explain that there is no active track"
    );
    assert!(
        !text.contains("Seeked to"),
        "seek without playback should not report a fake playback position"
    );
}

#[test]
fn test_transport_controls_without_playback_show_feedback() {
    let mut app = make_app(None, None);

    app.execute(Action::StopPlayback);
    app.execute(Action::NextTrack);
    app.execute(Action::PreviousTrack);
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Nothing playing"),
        "transport controls without playback should explain that no track is active"
    );
    assert!(
        !text.contains("Stopped playback"),
        "stop without playback should not claim playback was stopped"
    );
    assert!(
        !text.contains("No current track"),
        "next/previous without playback should not report a low-level empty track state"
    );
}

#[test]
fn test_empty_queue_view_shows_guidance() {
    let mut app = make_app(None, None);

    app.execute(Action::ShowQueue);

    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Queue (0 tracks)"));
    assert!(
        text.contains("Queue is empty"),
        "empty queue view should explain why no tracks are listed"
    );
    assert!(
        text.contains("Play or enqueue a song"),
        "empty queue view should tell users how to fill it"
    );
}

#[test]
fn test_empty_history_view_shows_guidance() {
    let mut app = make_app(None, None);

    dispatch_key(&mut app, make_key(KeyCode::Char('H')));

    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Recently Played (0)"));
    assert!(
        text.contains("No recently played tracks"),
        "empty history view should explain why no tracks are listed"
    );
    assert!(
        text.contains("Play a song to fill history"),
        "empty history view should tell users how to fill it"
    );
}

#[test]
fn test_history_view_records_recent_tracks_and_replays_selection() {
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("history-view")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;

    app.execute(Action::PlaySelected);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();

    dispatch_key(&mut app, make_key(KeyCode::Char('H')));

    let history = app.center_panel.get_history_songs();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].title, "Second Song");
    assert_eq!(history[1].title, "First Song");

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Recently Played (2)"));
    assert!(
        text.contains("1. Second Artist - Second Song"),
        "history should show the most recent track first"
    );
    assert!(
        text.contains("2. First Artist - First Song"),
        "history should keep older tracks below newer tracks"
    );

    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 3);
    assert_eq!(played[2].title, "First Song");
    drop(played);

    let history = app.center_panel.get_history_songs();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].title, "First Song");
    assert_eq!(history[1].title, "Second Song");
}

#[test]
fn test_history_persists_across_app_instances() {
    let dir = test_dir("persistent-history");
    let history_store = HistoryStore::with_path(dir.join("history.toml"));
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("persistent-history-playlists-1")),
        history_store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Persisted Song".to_string(),
            artist: "Persisted Artist".to_string(),
            path: PathBuf::from("/music/persisted.flac"),
            ..Default::default()
        }],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.tick();

    let (player, played, _enqueued) = MockPlayer::new();
    let mut restarted = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("persistent-history-playlists-2")),
        history_store,
        Box::new(player),
    );

    dispatch_key(&mut restarted, make_key(KeyCode::Char('H')));

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| restarted.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Recently Played (1)"));
    assert!(
        text.contains("Persisted Artist - Persisted Song"),
        "recently played history should load from disk on the next app instance"
    );

    restarted.execute(Action::PlaySelected);
    restarted.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Persisted Song");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_history_remove_and_clear_persist_across_app_instances() {
    let dir = test_dir("history-remove-clear");
    let history_store = HistoryStore::with_path(dir.join("history.toml"));
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("history-remove-clear-playlists-1")),
        history_store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();

    dispatch_key(&mut app, make_key(KeyCode::Char('H')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let history = app.center_panel.get_history_songs();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].title, "Second Song");

    let backend = TestBackend::new(520, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Recently Played (1)"));
    assert!(
        text.contains("Removed First Artist - First Song from history"),
        "removing a history row should identify the removed track"
    );
    assert!(
        !text.contains("1. First Artist - First Song"),
        "removed history row should no longer be visible"
    );

    let (player, _played, _enqueued) = MockPlayer::new();
    let mut restarted = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("history-remove-clear-playlists-2")),
        history_store.clone(),
        Box::new(player),
    );
    dispatch_key(&mut restarted, make_key(KeyCode::Char('H')));

    let history = restarted.center_panel.get_history_songs();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].title, "Second Song");

    restarted.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    restarted.tick();

    let history = restarted.center_panel.get_history_songs();
    assert!(history.is_empty());

    let frame = terminal.draw(|frame| restarted.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Recently Played (0)"));
    assert!(
        text.contains("Cleared 1 history track"),
        "clearing history should report the removed count"
    );

    let (player, _played, _enqueued) = MockPlayer::new();
    let mut restarted_again = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("history-remove-clear-playlists-3")),
        history_store,
        Box::new(player),
    );
    dispatch_key(&mut restarted_again, make_key(KeyCode::Char('H')));
    assert!(restarted_again.center_panel.get_history_songs().is_empty());

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_history_filter_removes_matching_original_track() {
    let dir = test_dir("history-filter-remove");
    let history_store = HistoryStore::with_path(dir.join("history.toml"));
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("history-filter-remove-playlists-1")),
        history_store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
            Song {
                title: "Third Song".to_string(),
                artist: "Third Artist".to_string(),
                path: PathBuf::from("/music/third.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.execute(Action::PlaySelected);
    app.tick();

    dispatch_key(&mut app, make_key(KeyCode::Char('H')));
    dispatch_key(&mut app, make_key(KeyCode::Char('f')));
    for key in "first".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Char(key)));
    }

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Filter History"));
    assert!(text.contains("Recently Played (1/3 matches)"));
    assert!(text.contains("3. First Artist - First Song"));
    assert!(
        !text.contains("2. Second Artist - Second Song"),
        "filtered history should hide nonmatching rows"
    );

    dispatch_key(&mut app, make_key(KeyCode::Enter));
    dispatch_key(&mut app, make_key(KeyCode::Char('d')));
    app.tick();

    let history = app.center_panel.get_history_songs();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].title, "Third Song");
    assert_eq!(history[1].title, "Second Song");

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Recently Played (0/2 matches)"));

    let (player, _played, _enqueued) = MockPlayer::new();
    let mut restarted = App::new_for_test_with_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("history-filter-remove-playlists-2")),
        history_store,
        Box::new(player),
    );
    dispatch_key(&mut restarted, make_key(KeyCode::Char('H')));

    let history = restarted.center_panel.get_history_songs();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].title, "Third Song");
    assert_eq!(history[1].title, "Second Song");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_playback_controls_show_feedback() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("playback-control-feedback")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::VolumeUp(5));
    app.execute(Action::ToggleShuffle);
    app.execute(Action::CycleRepeat);
    app.tick();

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Volume 5%"),
        "volume controls should show the resulting volume"
    );
    assert!(
        text.contains("Shuffle On"),
        "shuffle toggle should show the resulting shuffle state"
    );
    assert!(
        text.contains("Repeat All"),
        "repeat cycling should show the resulting repeat mode"
    );

    app.execute(Action::TogglePause);
    app.execute(Action::TogglePause);
    app.execute(Action::SeekForward(5.0));
    app.execute(Action::NextTrack);
    app.execute(Action::PreviousTrack);
    app.execute(Action::StopPlayback);
    app.tick();

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Paused"),
        "pause should show the resulting playback state"
    );
    assert!(
        text.contains("Playing"),
        "resuming should show the resulting playback state"
    );
    assert!(
        text.contains("Seeked to 0:05"),
        "seek should show the resulting playback position"
    );
    assert!(
        text.contains("Playing Second Song"),
        "next should show the resulting current track"
    );
    assert!(
        text.contains("Playing First Song"),
        "previous should show the resulting current track"
    );
    assert!(
        text.contains("Stopped playback"),
        "stop should confirm playback stopped"
    );
}

#[test]
fn test_global_transport_shortcuts_work_while_browsing() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("global-transport-shortcuts")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);

    dispatch_key(&mut app, make_key(KeyCode::Char('x')));
    dispatch_key(&mut app, make_key(KeyCode::Char('x')));
    dispatch_key(&mut app, make_key(KeyCode::Char('.')));
    dispatch_key(&mut app, make_key(KeyCode::Char(',')));
    dispatch_key(&mut app, make_key(KeyCode::Char('n')));
    dispatch_key(&mut app, make_key(KeyCode::Char('p')));
    dispatch_key(&mut app, make_key(KeyCode::Char('v')));
    app.tick();

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Paused"),
        "global pause shortcut should work without focusing the right panel"
    );
    assert!(
        text.contains("Playing"),
        "global pause shortcut should resume playback without focusing the right panel"
    );
    assert!(
        text.contains("Seeked to 0:05"),
        "global seek-forward shortcut should work without focusing the right panel"
    );
    assert!(
        text.contains("Seeked to 0:00"),
        "global seek-backward shortcut should work without focusing the right panel"
    );
    assert!(
        text.contains("Playing Second Song"),
        "global next shortcut should keep working while browsing"
    );
    assert!(
        text.contains("Playing First Song"),
        "global previous shortcut should keep working while browsing"
    );
    assert!(
        text.contains("Stopped playback"),
        "global stop shortcut should work without focusing the right panel"
    );
}

#[test]
fn test_toggle_mute_zeroes_and_restores_previous_volume() {
    let (mut player, _played, _enqueued) = MockPlayer::new();
    player.info.volume = 37;
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("toggle-mute")),
        Box::new(player),
    );

    app.execute(Action::ToggleMute);

    assert_eq!(app.player.get_playback_info().volume, 0);

    app.execute(Action::ToggleMute);

    assert_eq!(app.player.get_playback_info().volume, 37);

    app.tick();
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Muted"), "muting should log clear feedback");
    assert!(
        text.contains("Volume 37%"),
        "unmuting should log the restored volume"
    );
}

#[test]
fn test_seek_feedback_clamps_to_track_duration() {
    let (mut player, _played, _enqueued) = MockPlayer::new();
    player.info = PlaybackInfo {
        state: PlaybackState::Playing,
        current_song: Some(Song {
            title: "Final Minute".to_string(),
            path: PathBuf::from("/music/final-minute.flac"),
            ..Default::default()
        }),
        position: 58.0,
        duration: 60.0,
        ..Default::default()
    };
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("seek-clamps-feedback")),
        Box::new(player),
    );

    app.execute(Action::SeekForward(5.0));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Seeked to 1:00"),
        "seek feedback should report the clamped end-of-track position"
    );
    assert!(
        !text.contains("Seeked to 1:03"),
        "seek feedback should not report a position beyond the known duration"
    );
}

#[test]
fn test_queue_view_updates_after_next_track_action() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-next-sync")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("> 1. First Song"));

    app.execute(Action::NextTrack);
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("> 2. Second Song"),
        "queue view should refresh after playback advances"
    );
}

#[test]
fn test_queue_persists_across_app_instances_without_autoplay() {
    let dir = test_dir("persistent-queue");
    let queue_store = QueueStore::with_path(dir.join("queue.toml"));
    let history_store = HistoryStore::with_path(dir.join("history.toml"));
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-1")),
        history_store.clone(),
        queue_store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                duration_secs: Some(185.0),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                duration_secs: Some(190.0),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);

    let (player, played, _enqueued) = MockPlayer::new();
    let mut restarted = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-2")),
        history_store,
        queue_store,
        Box::new(player),
    );
    restarted.execute(Action::ShowQueue);

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| restarted.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        played.lock().unwrap().is_empty(),
        "restoring a queue should not start playback"
    );
    assert!(text.contains("Queue (2 tracks) - 6:15"));
    assert!(text.contains("> 1. First Artist - First Song"));
    assert!(text.contains("  2. Second Artist - Second Song"));
    assert!(text.contains("Not Playing"));

    restarted.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    dispatch_key(&mut restarted, make_key(KeyCode::Char(' ')));
    restarted.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 1);
    assert_eq!(played[0].title, "Second Song");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_queue_position_persists_after_next_track_action() {
    let dir = test_dir("persistent-queue-position");
    let queue_store = QueueStore::with_path(dir.join("queue.toml"));
    let history_store = HistoryStore::with_path(dir.join("history.toml"));
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-1")),
        history_store.clone(),
        queue_store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::NextTrack);

    let (player, _played, _enqueued) = MockPlayer::new();
    let mut restarted = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-2")),
        history_store,
        queue_store,
        Box::new(player),
    );
    restarted.execute(Action::ShowQueue);

    let backend = TestBackend::new(140, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| restarted.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("  1. First Song"));
    assert!(text.contains("> 2. Second Song"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_queue_remove_and_clear_persist_across_app_instances() {
    let dir = test_dir("persistent-queue-remove-clear");
    let queue_store = QueueStore::with_path(dir.join("queue.toml"));
    let history_store = HistoryStore::with_path(dir.join("history.toml"));
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-1")),
        history_store.clone(),
        queue_store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
            Song {
                title: "Third Song".to_string(),
                path: PathBuf::from("/music/third.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let (player, _played, _enqueued) = MockPlayer::new();
    let mut restarted = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-2")),
        history_store.clone(),
        queue_store.clone(),
        Box::new(player),
    );
    restarted.execute(Action::ShowQueue);

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| restarted.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Queue (2 tracks)"));
    assert!(text.contains("> 1. First Song"));
    assert!(text.contains("  2. Third Song"));
    assert!(
        !text.contains("Second Song"),
        "removed queue rows should stay removed after restart"
    );

    restarted.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    restarted.tick();

    let (player, _played, _enqueued) = MockPlayer::new();
    let mut restarted_again = App::new_for_test_with_all_stores_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(dir.join("playlists-3")),
        history_store,
        queue_store,
        Box::new(player),
    );
    restarted_again.execute(Action::ShowQueue);

    let frame = terminal
        .draw(|frame| restarted_again.render(frame))
        .unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Queue (1 track)"));
    assert!(text.contains("> 1. First Song"));
    assert!(
        !text.contains("Third Song"),
        "cleared upcoming queue rows should stay cleared after restart"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_queue_view_shows_total_duration_when_available() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-total-duration")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                duration_secs: Some(185.0),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                duration_secs: Some(190.0),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Queue (2 tracks) - 6:15"),
        "queue title should summarize total known duration"
    );
}

#[test]
fn test_space_in_queue_view_jumps_to_selected_queue_track() {
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-space-jump")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    dispatch_key(&mut app, make_key(KeyCode::Char(' ')));
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 2);
    assert_eq!(played[1].title, "Second Song");
}

#[test]
fn test_queue_filter_jumps_to_matching_original_track() {
    let (player, played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-filter-jump")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
            Song {
                title: "Needle Track".to_string(),
                artist: "Target Artist".to_string(),
                path: PathBuf::from("/music/needle.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);

    dispatch_key(&mut app, make_key(KeyCode::Char('f')));
    for key in "needle".chars() {
        dispatch_key(&mut app, make_key(KeyCode::Char(key)));
    }

    let backend = TestBackend::new(140, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Filter Queue"));
    assert!(text.contains("Queue (1/3 matches)"));
    assert!(text.contains("  3. Target Artist - Needle Track"));
    assert!(
        !text.contains("Second Song"),
        "filtered queue should hide nonmatching rows"
    );

    dispatch_key(&mut app, make_key(KeyCode::Enter));
    dispatch_key(&mut app, make_key(KeyCode::Char(' ')));
    app.tick();

    let played = played.lock().unwrap();
    assert_eq!(played.len(), 2);
    assert_eq!(played[1].title, "Needle Track");
}

#[test]
fn test_queue_remove_current_track_shows_feedback() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-remove-current")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let backend = TestBackend::new(240, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Cannot remove: First Artist - First Song"),
        "removing the current queue item should identify the protected track"
    );
    assert!(text.contains("> 1. First Artist - First Song"));
    assert!(text.contains("  2. Second Song"));
}

#[test]
fn test_queue_remove_upcoming_track_shows_track_feedback() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-remove-upcoming")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();

    let backend = TestBackend::new(320, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("Removed Second Artist - Second Song from queue"),
        "removing an upcoming queue item should confirm which track was removed"
    );
    assert!(text.contains("> 1. First Song"));
    assert!(
        !text.contains("  Second Artist - Second Song"),
        "removed queue item should no longer be listed"
    );
}

#[test]
fn test_queue_move_upcoming_track_down_updates_order_and_selection() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-move-down")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                duration_secs: Some(185.0),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                duration_secs: Some(190.0),
                ..Default::default()
            },
            Song {
                title: "Third Song".to_string(),
                path: PathBuf::from("/music/third.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('J')));
    app.tick();

    let queue_titles: Vec<_> = app
        .player
        .get_queue()
        .iter()
        .map(|song| song.title.as_str())
        .collect();
    assert_eq!(
        queue_titles,
        vec!["First Song", "Third Song", "Second Song"]
    );
    assert_eq!(
        app.center_panel.selected_songs_for_playlist()[0].title,
        "Second Song",
        "queue selection should follow the moved track"
    );

    let backend = TestBackend::new(280, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Moved Second Artist - Second Song down"),
        "moving a queued track should show feedback"
    );
}

#[test]
fn test_queue_move_current_track_is_blocked() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-move-current")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('J')));
    app.tick();

    let queue_titles: Vec<_> = app
        .player
        .get_queue()
        .iter()
        .map(|song| song.title.as_str())
        .collect();
    assert_eq!(queue_titles, vec!["First Song", "Second Song"]);

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Cannot move: First Artist - First Song"),
        "moving the current queue item should identify the protected track"
    );
}

#[test]
fn test_save_queue_to_existing_playlist_preserves_current_order() {
    let dir = test_dir("save-queue-existing-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Saved Queue".to_string()).unwrap();
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                duration_secs: Some(185.0),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                duration_secs: Some(190.0),
                ..Default::default()
            },
            Song {
                title: "Third Song".to_string(),
                path: PathBuf::from("/music/third.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('J')));
    app.tick();

    app.delegate_key_to_panel(make_key(KeyCode::Char('S')));
    app.tick();
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(
        playlists[0]
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["First Song", "Third Song", "Second Song"]
    );

    let backend = TestBackend::new(280, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added 3 tracks to 'Saved Queue'"),
        "saving queue to an existing playlist should use multi-track add feedback"
    );
    assert!(
        text.contains("Queue (3 tracks)"),
        "saving queue should return to the queue view after the picker closes"
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Saved Queue (3 tracks) - 6:15"),
        "reopened saved playlists should keep persisted track durations in the title"
    );
    assert!(
        text.contains("First Song (3:05)"),
        "reopened saved playlists should keep persisted row durations"
    );
    assert!(
        text.contains("Second Artist - Second Song (3:10)"),
        "reopened saved playlists should keep artist and duration context together"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_favorite_selected_song_creates_favorites_and_skips_duplicate() {
    let dir = test_dir("favorite-selected-song");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Favorite Song".to_string(),
                artist: "Favorite Artist".to_string(),
                path: PathBuf::from("/music/favorite.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    dispatch_key(&mut app, make_key(KeyCode::Char('F')));
    dispatch_key(&mut app, make_key(KeyCode::Char('F')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Favorite Song");
    assert_eq!(
        playlists[0].tracks[0].path.as_deref(),
        Some("/music/favorite.flac")
    );

    dispatch_key(&mut app, make_key(KeyCode::Char('U')));
    dispatch_key(&mut app, make_key(KeyCode::Char('U')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert!(playlists[0].tracks.is_empty());

    let backend = TestBackend::new(520, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added Favorite Artist - Favorite Song to Favorites"),
        "favoriting should confirm the selected track"
    );
    assert!(
        text.contains("Favorite Artist - Favorite Song is already in Favorites"),
        "favoriting the same track twice should report a duplicate"
    );
    assert!(
        text.contains("Removed Favorite Artist - Favorite Song from Favorites"),
        "unfavoriting should confirm the selected track"
    );
    assert!(
        text.contains("Favorite Artist - Favorite Song is not in Favorites"),
        "unfavoriting the same track twice should report a missing Favorites entry"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_favorite_selected_song_marks_open_album_row() {
    let dir = test_dir("favorite-selected-song-marker");
    let store = PlaylistStore::with_dir(dir.clone());
    let mut app =
        App::new_for_test_with_playlist_store(default_config(), None, None, store.clone());

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Favorite Song".to_string(),
                artist: "Favorite Artist".to_string(),
                path: PathBuf::from("/music/favorite.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    dispatch_key(&mut app, make_key(KeyCode::Char('F')));

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Favorite Artist - Favorite Song [fav]"),
        "favorited open-album rows should be marked"
    );
    assert!(
        !text.contains("First Artist - First Song [fav]"),
        "non-favorited rows should not be marked"
    );

    dispatch_key(&mut app, make_key(KeyCode::Char('U')));

    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("Favorite Artist - Favorite Song [fav]"),
        "unfavorited rows should lose the favorite marker"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_favorite_current_track_from_right_panel() {
    let dir = test_dir("favorite-current-track");
    let store = PlaylistStore::with_dir(dir.clone());
    let (mut player, _played, _enqueued) = MockPlayer::new();
    player.info.current_song = Some(Song {
        title: "Now Playing".to_string(),
        artist: "Current Artist".to_string(),
        path: PathBuf::from("/music/current.flac"),
        ..Default::default()
    });
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );
    app.focused_window = FocusedWindow::Right;

    dispatch_key(&mut app, make_key(KeyCode::Char('F')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Now Playing");
    assert_eq!(playlists[0].tracks[0].artist, "Current Artist");

    dispatch_key(&mut app, make_key(KeyCode::Char('U')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert!(playlists[0].tracks.is_empty());

    let backend = TestBackend::new(420, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added Current Artist - Now Playing to Favorites"),
        "favoriting from the right panel should use the current track"
    );
    assert!(
        text.contains("Removed Current Artist - Now Playing from Favorites"),
        "unfavoriting from the right panel should use the current track"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_add_selected_queue_track_to_existing_playlist() {
    let dir = test_dir("queue-track-to-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Favorites".to_string()).unwrap();
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                artist: "Second Artist".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    app.execute(Action::AddToPlaylist);
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Second Song");
    assert_eq!(playlists[0].tracks[0].artist, "Second Artist");

    let backend = TestBackend::new(360, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added Second Artist - Second Song to 'Favorites'"),
        "adding a queue row to a playlist should identify the saved track"
    );
    assert!(
        text.contains("Queue (2 tracks)"),
        "adding a queue row to a playlist should return to the queue view"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_favorite_selected_queue_track() {
    let dir = test_dir("favorite-queue-track");
    let store = PlaylistStore::with_dir(dir.clone());
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                artist: "First Artist".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Queue Favorite".to_string(),
                artist: "Queue Artist".to_string(),
                path: PathBuf::from("/music/queue-favorite.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('j')));

    dispatch_key(&mut app, make_key(KeyCode::Char('F')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert_eq!(playlists[0].tracks.len(), 1);
    assert_eq!(playlists[0].tracks[0].title, "Queue Favorite");
    assert_eq!(playlists[0].tracks[0].artist, "Queue Artist");

    dispatch_key(&mut app, make_key(KeyCode::Char('U')));

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Favorites");
    assert!(playlists[0].tracks.is_empty());

    let backend = TestBackend::new(420, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added Queue Artist - Queue Favorite to Favorites"),
        "favoriting a queue row should identify the selected queue track"
    );
    assert!(
        text.contains("Queue (2 tracks)"),
        "favoriting a queue row should stay in the queue view"
    );
    assert!(
        text.contains("Removed Queue Artist - Queue Favorite from Favorites"),
        "unfavoriting a queue row should identify the selected queue track"
    );
    assert!(
        text.contains("Queue (2 tracks)"),
        "unfavoriting a queue row should stay in the queue view"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_save_queue_creates_playlist_when_none_exist() {
    let dir = test_dir("save-queue-new-playlist");
    let store = PlaylistStore::with_dir(dir.clone());
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        store.clone(),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);

    app.delegate_key_to_panel(make_key(KeyCode::Char('S')));
    app.tick();
    for c in "Road Queue".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let playlists = store.load_all();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Road Queue");
    assert_eq!(
        playlists[0]
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["First Song", "Second Song"]
    );

    let backend = TestBackend::new(260, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Added 2 tracks to 'Road Queue'"),
        "new queue playlist should be created and populated"
    );
    assert!(
        text.contains("Queue (2 tracks)"),
        "creating a playlist from queue should return to the queue view"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_save_empty_queue_shows_feedback() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("save-empty-queue")),
        Box::new(player),
    );

    app.execute(Action::ShowQueue);
    app.delegate_key_to_panel(make_key(KeyCode::Char('S')));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("No queued tracks to save"),
        "empty queue save should explain why no playlist picker opened"
    );
    assert!(
        text.contains("Queue is empty"),
        "empty queue view should remain visible"
    );
}

#[test]
fn test_queue_clear_removes_upcoming_tracks() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-clear-upcoming")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);

    app.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Cleared 1 queued track"));
    assert!(
        !text.contains("track(s)"),
        "queue clear feedback should use normal singular/plural wording"
    );
    assert!(text.contains("Queue (1 track)"));
    assert!(text.contains("> 1. First Song"));
    assert!(
        !text.contains("Second Song"),
        "clearing the queue should remove upcoming tracks while keeping the current track"
    );
}

#[test]
fn test_queue_clear_uses_plural_feedback_for_multiple_tracks() {
    let (player, _played, _enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        None,
        None,
        PlaylistStore::with_dir(test_dir("queue-clear-multiple")),
        Box::new(player),
    );

    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![
            Song {
                title: "First Song".to_string(),
                path: PathBuf::from("/music/first.flac"),
                ..Default::default()
            },
            Song {
                title: "Second Song".to_string(),
                path: PathBuf::from("/music/second.flac"),
                ..Default::default()
            },
            Song {
                title: "Third Song".to_string(),
                path: PathBuf::from("/music/third.flac"),
                ..Default::default()
            },
        ],
    );
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::PlaySelected);
    app.execute(Action::ShowQueue);

    app.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Cleared 2 queued tracks"));
    assert!(
        !text.contains("track(s)"),
        "queue clear feedback should not expose placeholder pluralization"
    );
    assert!(text.contains("Queue (1 track)"));
    assert!(text.contains("> 1. First Song"));
}

#[test]
fn test_enqueue_streaming_playlist_track_resolves_before_player() {
    let dir = test_dir("enqueue-streaming-playlist-track");
    let store = PlaylistStore::with_dir(dir.clone());
    store.create("Streams".to_string()).unwrap();
    store
        .add_songs_to_index(
            0,
            &[Song {
                title: "Saved Stream".to_string(),
                artist: "Artist".to_string(),
                album_name: "Album".to_string(),
                stream_service: Some("Qobuz".to_string()),
                stream_track_id: Some("2".to_string()),
                ..Default::default()
            }],
        )
        .unwrap();

    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let (player, _played, enqueued) = MockPlayer::new();
    let mut app = App::new_for_test_with_playlist_store_and_player(
        default_config(),
        Some(Box::new(mock)),
        None,
        store.clone(),
        Box::new(player),
    );

    switch_to_tab(&mut app, "Playlists");
    app.execute(Action::SelectAlbum);
    app.focused_window = FocusedWindow::Center;
    app.execute(Action::EnqueueSelected);
    app.tick();

    let enqueued = enqueued.lock().unwrap();
    assert_eq!(enqueued.len(), 1);
    assert_eq!(enqueued[0].title, "Saved Stream");
    assert_eq!(enqueued[0].artist, "Artist");
    assert_eq!(enqueued[0].album_name, "Album");
    assert_eq!(enqueued[0].stream_service.as_deref(), Some("Qobuz"));
    assert_eq!(enqueued[0].stream_track_id.as_deref(), Some("2"));
    assert_eq!(
        enqueued[0].url.as_deref(),
        Some("https://example.com/stream.flac")
    );
    drop(enqueued);

    let backend = TestBackend::new(220, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Queued Artist - Saved Stream"),
        "enqueueing a resolved streaming track should confirm which track was queued"
    );

    let _ = std::fs::remove_dir_all(dir);
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

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("No local sources"),
        "empty Local search should explain that a source must be configured"
    );
    assert!(
        text.contains("Settings"),
        "empty Local search should point first-run users to Settings"
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
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Albums (0)"),
        "No results yet while auth is pending"
    );
    assert!(
        text.contains("Albums (0) - Please authorize at: https://example.com"),
        "pending Tidal auth should be visible in the main search panel"
    );
    assert!(
        text.contains("Login Required"),
        "pending Tidal auth should show the login popup"
    );
    assert!(
        !text.contains("Albums (0) - Searching Tidal"),
        "pending auth should replace stale searching status"
    );

    dispatch_key(&mut app, make_key(KeyCode::Esc));
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("Login Required"),
        "Esc should dismiss the Tidal login popup"
    );
    assert!(
        text.contains("Please authorize at: https://example.com"),
        "dismissing the popup should keep the Tidal auth message visible"
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
fn test_account_clear_cancels_pending_tidal_auth() {
    let mock = MockStreamingService::new_pending("Tidal", 1, mock_albums());
    let mut app = make_app(None, Some(Box::new(mock)));

    switch_to_tab(&mut app, "Tidal");
    app.execute(Action::OpenSearch);
    for c in "hello".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));

    app.tick();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    app.tick();
    app.execute(Action::ToggleSettings);

    for _ in 0..3 {
        app.tick();
    }

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        !text.contains("Artist1 - Album1"),
        "clearing accounts should cancel the deferred Tidal search"
    );
    assert!(
        !text.contains("Albums (2)"),
        "cleared Tidal auth should not complete and render search results"
    );
}

#[test]
fn test_account_clear_cancels_inflight_streaming_search() {
    let mock = MockStreamingService::new_authenticated_slow("Qobuz", mock_albums(), 300);
    let mut app = make_app(Some(Box::new(mock)), None);

    switch_to_tab(&mut app, "Qobuz");
    app.execute(Action::OpenSearch);
    for c in "slow".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    app.tick();
    app.execute(Action::ToggleSettings);

    std::thread::sleep(Duration::from_millis(380));
    app.tick();

    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        !text.contains("Artist1 - Album1"),
        "clearing accounts should discard in-flight streaming search results"
    );
    assert!(
        !text.contains("Albums (2)"),
        "cleared streaming accounts should not render stale search results"
    );
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
    // Left panel top-left border corner should use the high-contrast focus color.
    assert_eq!(
        frame.buffer.cell((0, 0)).unwrap().fg,
        theme::FOCUS,
        "Focused left panel border should use the theme focus color"
    );

    // Tab → Center
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Center);
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    // Left panel border no longer uses the focus color.
    assert_ne!(
        frame.buffer.cell((0, 0)).unwrap().fg,
        theme::FOCUS,
        "Unfocused left panel border should not use the focus color"
    );

    // Tab → Right
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Right);
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    assert_eq!(
        frame.buffer.cell((64, 0)).unwrap().fg,
        theme::FOCUS,
        "Right focus should highlight the playback panel"
    );
    assert_ne!(
        frame.buffer.cell((64, 18)).unwrap().fg,
        theme::FOCUS,
        "Right focus should not highlight the logs panel"
    );

    // Tab → Logs
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Logs);
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    assert_ne!(
        frame.buffer.cell((64, 0)).unwrap().fg,
        theme::FOCUS,
        "Logs focus should not highlight the playback panel"
    );
    assert_eq!(
        frame.buffer.cell((64, 18)).unwrap().fg,
        theme::FOCUS,
        "Logs focus should highlight the logs panel"
    );

    // Tab → wraps back to Left
    app.execute(Action::SwitchPanel);
    assert_eq!(app.focused_window, FocusedWindow::Left);
}

#[test]
fn test_logs_panel_clear_shortcut_removes_visible_history() {
    let mut app = make_app(None, None);
    app.execute(Action::TogglePause);
    app.focused_window = FocusedWindow::Logs;

    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("Nothing playing"));

    dispatch_key(&mut app, make_key(KeyCode::Char('c')));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(text.contains("No logs yet"));
    assert!(
        !text.contains("Nothing playing"),
        "clearing logs should remove previous visible messages"
    );
}

#[test]
fn test_settings_opens_and_closes() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(120, 24);
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
    assert!(
        text.contains("Shuffle: Off"),
        "General settings should show startup shuffle"
    );
    assert!(
        text.contains("Repeat: Off"),
        "General settings should show startup repeat"
    );

    // Close with Esc
    app.delegate_key_to_panel(make_key(KeyCode::Esc));
    assert!(!app.settings_panel.opened);

    // Render and verify settings overlay is gone
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        !text.contains("General"),
        "Settings overlay should not be visible after closing"
    );
}

#[test]
fn test_settings_startup_playback_modes_update_config() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(120, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    dispatch_key(&mut app, make_key(KeyCode::Char('z')));
    app.tick();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Shuffle: On"),
        "startup shuffle should update in settings after z"
    );

    dispatch_key(&mut app, make_key(KeyCode::Char('r')));
    app.tick();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Repeat: All"),
        "startup repeat should cycle to All after r"
    );

    dispatch_key(&mut app, make_key(KeyCode::Char('r')));
    app.tick();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Repeat: One"),
        "startup repeat should cycle to One after a second r"
    );
}

#[test]
fn test_settings_duplicate_local_source_shows_feedback() {
    let dir = test_dir("duplicate-local-source");
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('a')));
    for c in "Duplicate".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in dir.to_string_lossy().chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Source already exists"),
        "Duplicate local source should show feedback"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn test_removing_open_local_source_clears_center_album() {
    let dir = test_dir("remove-open-local-source");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("source-track.flac"), "").unwrap();
    let config = Config {
        local: LocalConfig {
            sources: vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        },
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::SelectAlbum);
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("source-track.flac"),
        "the local source should be open before removal"
    );

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Char('d')));
    app.tick();
    app.delegate_key_to_panel(make_key(KeyCode::Esc));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("No local sources"),
        "removed local source should disappear from the left panel"
    );
    assert!(
        text.contains("Select an album or playlist"),
        "center panel should return to guidance after removing the opened local source"
    );
    assert!(
        !text.contains("source-track.flac"),
        "removed source tracks should not remain visible in the center panel"
    );

    let _ = std::fs::remove_dir_all(dir);
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
fn test_account_settings_qobuz_status() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(100, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Not configured"),
        "Qobuz without credentials should show 'Not configured'"
    );

    let config = Config {
        local: LocalConfig {
            sources: Vec::new(),
        },
        qobuz: Some(QobuzConfig {
            email: "listener@example.com".to_string(),
            password: "secret".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        tidal: None,
        audio: AudioConfig {
            default_volume: 50,
            max_stream_quality: MaxStreamQuality::HiRes,
            default_shuffle: ShuffleMode::Off,
            default_repeat: RepeatMode::Off,
        },
    };
    let mut app = App::new_for_test(config, None, None);
    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Configured"),
        "Qobuz with credentials should show 'Configured'"
    );
}

#[test]
fn test_account_settings_whitespace_qobuz_is_not_configured() {
    let config = Config {
        qobuz: Some(QobuzConfig {
            email: "   ".to_string(),
            password: "   ".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(100, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Not configured"),
        "whitespace-only Qobuz credentials should not be shown as configured"
    );
}

#[test]
fn test_account_edit_masks_password_until_password_field_active() {
    let config = Config {
        qobuz: Some(QobuzConfig {
            email: "listener@example.com".to_string(),
            password: "super-secret".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('e')));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("listener@example.com"),
        "email field should remain visible while editing account settings"
    );
    assert!(
        !text.contains("super-secret"),
        "password should stay masked while the email field is active"
    );

    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("super-secret"),
        "password should be visible only while the password field is active"
    );
}

#[test]
fn test_account_edit_appends_to_existing_email() {
    let config = Config {
        qobuz: Some(QobuzConfig {
            email: "listener@example.com".to_string(),
            password: "secret".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('e')));
    app.delegate_key_to_panel(make_key(KeyCode::Char('x')));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("listener@example.comx"),
        "typing in an existing account field should append at the end"
    );
    assert!(
        !text.contains("xlistener@example.com"),
        "existing account fields should not start with the cursor at column zero"
    );
}

#[test]
fn test_account_save_qobuz_shows_feedback() {
    let mut app = make_app(None, None);
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('e')));
    for c in "listener@example.com".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Tab));
    for c in "secret".chars() {
        app.delegate_key_to_panel(make_key(KeyCode::Char(c)));
    }
    app.delegate_key_to_panel(make_key(KeyCode::Enter));
    app.tick();

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Qobuz account saved"),
        "saving Qobuz credentials should show account feedback"
    );
    assert!(
        text.contains("Configured"),
        "saved Qobuz credentials should immediately update account status"
    );
}

#[test]
fn test_account_settings_can_check_qobuz_login() {
    let config = Config {
        qobuz: Some(QobuzConfig {
            email: "listener@example.com".to_string(),
            password: "secret".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        ..default_config()
    };
    let mock = MockStreamingService::new_authenticated("Qobuz", mock_albums());
    let mut app = App::new_for_test(config, Some(Box::new(mock)), None);
    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('q')));
    app.tick();

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Qobuz account verified"),
        "checking Qobuz login from settings should show visible account feedback"
    );
}

#[test]
fn test_account_clear_shows_feedback() {
    let config = Config {
        qobuz: Some(QobuzConfig {
            email: "listener@example.com".to_string(),
            password: "secret".to_string(),
            app_id: String::new(),
            app_secret: String::new(),
        }),
        tidal: Some(TidalConfig {
            access_token: "abc123".to_string(),
            refresh_token: "def456".to_string(),
            country_code: "US".to_string(),
            token_expiry: 1700000000,
        }),
        ..default_config()
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('c')));
    app.tick();

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Streaming accounts cleared"),
        "clearing accounts should show account feedback"
    );
    assert!(
        text.contains("Not configured"),
        "clearing accounts should update Qobuz status"
    );
    assert!(
        text.contains("Not authenticated"),
        "clearing accounts should update Tidal status"
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
        audio: AudioConfig {
            default_volume: 50,
            max_stream_quality: MaxStreamQuality::HiRes,
            default_shuffle: ShuffleMode::Off,
            default_repeat: RepeatMode::Off,
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
fn test_account_settings_can_start_tidal_login() {
    let mock = MockStreamingService::new_pending("Tidal", 2, mock_albums());
    let mut app = make_app(None, Some(Box::new(mock)));
    let backend = TestBackend::new(180, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.settings_panel.next_tab();
    app.delegate_key_to_panel(make_key(KeyCode::Char('t')));
    app.tick();

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Please authorize at: https://example.com"),
        "Tidal login from settings should show the device authorization message"
    );

    app.tick();

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Authenticated"),
        "completed Tidal login should update account status while settings remains open"
    );
}

#[test]
fn test_account_settings_whitespace_tidal_is_not_authenticated() {
    let config = Config {
        local: LocalConfig {
            sources: Vec::new(),
        },
        qobuz: None,
        tidal: Some(TidalConfig {
            access_token: "   ".to_string(),
            refresh_token: "def456".to_string(),
            country_code: "US".to_string(),
            token_expiry: 1700000000,
        }),
        audio: AudioConfig {
            default_volume: 50,
            max_stream_quality: MaxStreamQuality::HiRes,
            default_shuffle: ShuffleMode::Off,
            default_repeat: RepeatMode::Off,
        },
    };
    let mut app = App::new_for_test(config, None, None);
    let backend = TestBackend::new(80, 40);
    let mut terminal = Terminal::new(backend).unwrap();

    app.execute(Action::ToggleSettings);
    app.delegate_key_to_panel(make_key(KeyCode::Tab));

    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);
    assert!(
        text.contains("Not authenticated"),
        "whitespace-only Tidal access tokens should not be shown as authenticated"
    );
}

#[test]
fn test_keybind_tab_shows_generic_search_text() {
    let mut app = make_app(None, None);
    // Use a larger terminal so all keybinds are visible without clipping.
    let backend = TestBackend::new(160, 90);
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
        text.contains("Cycle search type"),
        "Search keybinds should document switching between album, artist, and track search"
    );
    assert!(
        text.contains("Space/Enter"),
        "Center keybinds should document that Enter plays selected songs"
    );
    assert!(
        !text.contains("Open search (Qobuz)"),
        "Should NOT show service-specific search text"
    );
    assert!(
        !text.contains("Open search (Tidal)"),
        "Should NOT show service-specific search text"
    );
    assert!(
        text.contains("Remove local source"),
        "Settings keybinds should document source removal"
    );
    assert!(
        text.contains("Edit local source"),
        "Settings keybinds should document source editing"
    );
    assert!(
        text.contains("Reorder selected source"),
        "Settings keybinds should document source reordering"
    );
    assert!(
        text.contains("Next / previous track"),
        "Keybinds should document global next/previous controls"
    );
    assert!(
        text.contains("Adjust volume"),
        "Keybinds should document global volume controls"
    );
    assert!(
        text.contains("Save current volume"),
        "Keybinds should document saving the runtime volume as startup volume"
    );
    assert!(
        text.contains("Show recently played"),
        "Keybinds should document recently played history"
    );
    assert!(
        text.contains("Play history track"),
        "Keybinds should document replaying recently played tracks"
    );
    assert!(
        text.contains("Remove history track"),
        "Keybinds should document history item removal"
    );
    assert!(
        text.contains("Clear history"),
        "Keybinds should document clearing recently played history"
    );
    assert!(
        text.contains("Filter history"),
        "Keybinds should document history filtering"
    );
    assert!(
        text.contains("Adjust startup volume"),
        "Settings keybinds should document startup volume controls"
    );
    assert!(
        text.contains("Toggle startup shuffle"),
        "Settings keybinds should document startup shuffle controls"
    );
    assert!(
        text.contains("Cycle startup repeat"),
        "Settings keybinds should document startup repeat controls"
    );
    assert!(
        text.contains("Filter queue"),
        "Settings keybinds should document queue filtering"
    );
    assert!(
        text.contains("Jump first / latest log"),
        "Settings keybinds should document log Home/End navigation"
    );
    assert!(
        text.contains("Log in to Tidal"),
        "Settings keybinds should document Tidal login"
    );
    assert!(
        text.contains("Check Qobuz login"),
        "Settings keybinds should document Qobuz login checks"
    );
    assert!(
        text.contains("Remove track from playlist"),
        "Keybinds should document playlist track removal"
    );
    assert!(
        text.contains("Favorites"),
        "Keybinds should document Favorites shortcuts"
    );
    assert!(
        text.contains("Move playlist track"),
        "Keybinds should document playlist track reordering"
    );
    assert!(
        text.contains("Add collection to playlist"),
        "Keybinds should document adding the open collection to a playlist"
    );
    assert!(
        text.contains("Add album/playlist to Favorites"),
        "Keybinds should document favoriting selected left-panel collections"
    );
    assert!(
        text.contains("Remove album/playlist from Favorites"),
        "Keybinds should document unfavoriting selected left-panel collections"
    );
    assert!(
        text.contains("Remove selected track from Favorites"),
        "Keybinds should document unfavoriting selected center-panel tracks"
    );
    assert!(
        text.contains("Remove current track from Favorites"),
        "Keybinds should document unfavoriting the now-playing track"
    );
    assert!(
        text.contains("Add current track"),
        "Keybinds should document adding the now-playing track to a playlist"
    );
    assert!(
        text.contains("Rename playlist"),
        "Keybinds should document playlist rename"
    );
    assert!(
        text.contains("Filter Local/Playlists list"),
        "Keybinds should document focused list filtering"
    );
    assert!(
        text.contains("Duplicate playlist"),
        "Keybinds should document playlist duplication"
    );
    assert!(
        text.contains("Jump to queue track"),
        "Keybinds should document queue jump behavior"
    );
    assert!(
        text.contains("Add queue track to playlist"),
        "Keybinds should document queue track playlist adds"
    );
    assert!(
        text.contains("Remove queue track from Favorites"),
        "Keybinds should document unfavoriting selected queue rows"
    );
    assert!(
        text.contains("Remove queue track"),
        "Keybinds should document queue removal behavior"
    );
    assert!(
        text.contains("Move queue track"),
        "Keybinds should document queue reordering"
    );
    assert!(
        text.contains("Save queue as playlist"),
        "Keybinds should document saving the current queue"
    );
    assert!(
        text.contains("Clear queued tracks"),
        "Keybinds should document queue clear behavior"
    );
    assert!(
        text.contains("Logs Panel"),
        "Keybinds should document the logs panel"
    );
    assert!(
        text.contains("Refresh library"),
        "Keybinds should document library refresh"
    );
    assert!(
        text.contains("Toggle pause"),
        "Keybinds should document global pause toggling"
    );
    assert!(
        text.contains("Stop playback"),
        "Keybinds should document global stop playback"
    );
    assert!(
        text.contains("Seek backward / forward 5s"),
        "Keybinds should document global seek shortcuts"
    );
    assert!(
        text.contains("Play album/playlist"),
        "Keybinds should document left-panel collection playback"
    );
    assert!(
        text.contains("Queue album/playlist"),
        "Keybinds should document left-panel collection queueing"
    );
    assert!(
        text.contains("Queue open collection"),
        "Keybinds should document center-panel collection queueing"
    );
    assert!(
        text.contains("Home / End"),
        "Keybinds should document first/last list navigation"
    );
    assert!(
        text.contains("PageUp / PageDown"),
        "Keybinds should document page-wise list navigation"
    );
    assert!(
        text.contains("Scroll log horizontally"),
        "Keybinds should document horizontal log scrolling"
    );
    assert!(
        text.contains("Clear logs"),
        "Keybinds should document clearing the logs panel"
    );
    assert!(
        text.contains("Jump cursor start / end"),
        "Keybinds should document text input cursor jumps"
    );
    assert!(
        text.contains("Delete text"),
        "Keybinds should document text deletion controls"
    );
    assert!(
        text.contains("Close active view / quit"),
        "Keybinds should document Esc close-or-quit behavior"
    );
    assert!(
        text.contains("Quit application"),
        "Keybinds should document quit behavior"
    );
    assert!(
        text.contains("Clear streaming accounts"),
        "Settings keybinds should document account clearing"
    );
}

#[test]
fn test_question_mark_opens_keybinds_help_directly() {
    let mut app = make_app(None, None);
    app.focused_window = FocusedWindow::Center;

    dispatch_key(&mut app, make_key(KeyCode::Char('?')));

    assert!(app.settings_panel.opened);
    assert_eq!(app.focused_window, FocusedWindow::Settings);

    let backend = TestBackend::new(100, 90);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|f| app.render(f)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Keybinds"));
    assert!(text.contains("Queue View"));
    assert!(text.contains("Open keybind help"));

    app.delegate_key_to_panel(make_key(KeyCode::Esc));

    assert!(!app.settings_panel.opened);
    assert_eq!(app.focused_window, FocusedWindow::Center);
}

#[test]
fn test_search_status_visible_while_background_query_runs() {
    let mock = MockStreamingService::new_authenticated_slow("Qobuz", mock_albums(), 300);
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
    std::thread::sleep(Duration::from_millis(380));
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
    let slow_query_started = Instant::now();
    app.tick();

    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let text = tick_until_rendered(
        &mut app,
        &mut terminal,
        Duration::from_millis(700),
        |text| text.contains("timed out"),
    );
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

    let text = tick_until_rendered(
        &mut app,
        &mut terminal,
        Duration::from_millis(900),
        |text| {
            slow_query_started.elapsed() >= Duration::from_millis(340)
                && text.contains("Artist - fast Album")
        },
    );
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
fn test_open_album_shows_disc_and_track_numbers() {
    let mut app = make_app(None, None);
    app.center_panel.set_album(
        PathBuf::from("/music/album"),
        vec![Song {
            title: "Second Movement".to_string(),
            artist: "Performer".to_string(),
            disc_number: Some(2),
            track_number: Some(4),
            ..Default::default()
        }],
    );

    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(
        text.contains("2.04 Performer - Second Movement"),
        "open album rows should expose disc and track metadata"
    );
}

#[test]
fn test_streaming_album_with_no_tracks_shows_guidance() {
    let mock =
        MockStreamingService::new_authenticated("Qobuz", mock_albums()).with_album_tracks(vec![]);
    let mut app = make_app(Some(Box::new(mock)), None);

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

    let backend = TestBackend::new(140, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    let frame = terminal.draw(|frame| app.render(frame)).unwrap();
    let text = extract_buffer_text(frame.buffer);

    assert!(text.contains("Artist1 - Album1 (0 tracks)"));
    assert!(
        text.contains("No tracks found"),
        "streaming albums with no returned tracks should explain the empty list"
    );
    assert!(
        text.contains("Press Esc to return to albums"),
        "empty streaming albums should show the back shortcut"
    );
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
