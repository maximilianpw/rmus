# Changelog

## v1.0.0 - 2026-06-24

First stable release of `rmus`, a keyboard-driven terminal music player for local libraries and Qobuz/Tidal streaming.

Highlights:

- Local music sources with responsive album discovery, cached metadata, whole-library playback, local filtering, and background cache warming for large folders.
- mpv-backed playback with queue persistence, history, shuffle/repeat, seek, volume, mute, favorites, and rich now-playing metadata.
- Persistent playlists with local tracks and saved Qobuz/Tidal references, including create, rename, duplicate, delete, import, export, reorder, and add-to-playlist workflows.
- Qobuz credential login and Tidal device-code authentication with login-required popups, deferred searches, and stream-quality selection.
- CLI diagnostics and maintenance commands for sources, local search, playlists, queue, history, cache, paths, completions, and account clearing.
- Homebrew installation through `maximilianpw/tap/rmus`.

Verification:

- `cargo fmt -- --check`
- `cargo clippy --all-features --all-targets -- -D warnings`
- `cargo test --locked --all-features --all-targets`
