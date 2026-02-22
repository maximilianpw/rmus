# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

rmus is a terminal-based music player inspired by cmus, built with Rust and Ratatui. It supports local file browsing and streaming from Qobuz and Tidal. MPV is used as the audio backend via IPC.

## Build & Development Commands

```bash
cargo build                                    # Debug build
cargo build --release                          # Release build (LTO, size-optimized, stripped)
cargo test --locked --all-features --all-targets  # Full test suite (CI command)
cargo test <test_name>                         # Run a single test
cargo fmt                                      # Format code
cargo fmt -- --check                           # Check formatting (CI command)
cargo clippy                                   # Lint
```

Nix flake provides the dev environment with Rust nightly toolchain and MPV. Use `nix develop` or direnv.

Runtime dependency: `mpv` must be available on PATH.

## Architecture

The app follows a classic TUI event loop pattern: poll input → resolve keymap → execute action → render.

### Event Flow

1. **Crossterm events** are polled with 50ms timeout in `app.rs::App::run`
2. **Keymap resolution** (`keymap.rs::resolve_key`) maps KeyEvent → `Action` enum, distinguishing global keys (quit, tab, settings, search) from panel-delegated keys
3. **Action execution** (`app.rs::App::execute`) updates state, controls the player, or manages panels
4. **Tick** (`app.rs::App::tick`) handles deferred async work: auth polling, config sync, pending searches

### Key Abstractions (Traits)

- **`MusicPlayer`** (`players/mod.rs`) — playback control (play, pause, seek, volume, next/prev). `SafePlayer` wraps `MpvPlayer` with path validation and secure socket setup.
- **`MusicSource`** (`sources/mod.rs`) — provides albums and songs. `LocalFiles` scans configured directories.
- **`StreamingService`** (`sources/streaming.rs`) — authentication, search, and stream URL resolution. Implemented by `QobuzSource` and `TidalSource`.
- **`AppPanel`** (`ui/mod.rs`) — rendering contract for UI panels.

### Module Layout

- `app.rs` — App struct (main state machine), event loop, action dispatch
- `action.rs` — Action enum (all user-triggerable actions)
- `keymap.rs` — keybinding resolution logic
- `config.rs` — TOML config loading/saving via XDG dirs (`~/.config/rmus/config.toml`)
- `players/mpv.rs` — MPV IPC implementation (JSON-RPC over Unix socket)
- `sources/qobuz.rs` — Qobuz API client (MD5 password auth)
- `sources/tidal.rs` — Tidal API client (OAuth2 device code flow with polling)
- `ui/` — panel implementations: LeftPanel (album browser), CenterPanel (songs/search), RightPanel (now-playing), LogPanel, SettingsPanel

### Async Pattern

Streaming services use tokio internally but expose sync trait methods (blocking at the trait boundary). Long-running auth flows (Tidal device code) use a poll model: `authenticate()` returns `PendingUserAction`, then `poll_auth()` is called each tick until complete. Searches can be deferred if auth is still pending.

### Testing

Tests are in `tests/e2e.rs` and `src/config.rs`. E2E tests use `App::new_for_test()` with a `MockStreamingService` and Ratatui's `TestBackend` for headless rendering verification. No MPV process is needed for tests.
