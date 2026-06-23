# rmus

`rmus` is a keyboard-driven terminal music player inspired by [cmus], built with Rust and [Ratatui].

It is focused on fast local-library playback with playlist and queue workflows, plus first-class Qobuz and Tidal search/playback support.

## Features

- Local music sources configured from the settings UI, with cached album-folder discovery under each source.
- Audio-file filtering, album-aware row labels, disc/track-aware ordering, and cached best-effort metadata reading via `lofty`.
- mpv-backed playback with pause, stop, next/previous, seek, volume/mute, shuffle, and repeat controls.
- Now-playing metadata for artist, album, disc, track, stream source, quality, and duration.
- High-contrast TUI styling for focused panes, selected rows, status text, and muted guidance.
- Numbered queue view with restart persistence, jump, reorder, remove, clear, playlist add, and save actions.
- Recently played history for quickly reopening prior tracks across app restarts.
- Persistent playlists with local tracks, saved Qobuz/Tidal track references, and duplicate/rename/delete workflows.
- Local `.m3u`/`.m3u8` playlist import and export from the CLI.
- Qobuz account configuration and stream-quality selection.
- Tidal device-code authentication with token persistence.
- Streaming album, artist, and track search with background requests and timeout handling.

## Requirements

- Rust toolchain with Cargo.
- `mpv` available on `PATH` for playback.

## Install

```sh
brew install maximilianpw/tap/rmus
```

## Run

```sh
cargo run
```

## Diagnostics

```sh
rmus doctor
rmus paths
```

The doctor command checks the installed `rmus` version, whether `mpv` is available on `PATH`, where config/playlists/history/queue/local-cache files are stored, and whether configured local source folders still exist.

The paths command prints only the app storage paths, which is useful when inspecting config, playlists, playback state, or the local metadata cache directly.

## Import Playlists

```sh
rmus import-playlist ~/Music/Mix.m3u
rmus import-playlist ~/Music/Mix.m3u "Road Mix"
rmus export-playlist "Road Mix" ~/Music/Road-Mix.m3u8
```

The import command creates a new rmus playlist from local file entries in `.m3u` or `.m3u8` files. Relative entries are resolved from the playlist file's folder, `#EXTINF` titles and durations are preserved when present, and URL entries are skipped.

The export command writes local tracks from an rmus playlist to `.m3u8`, including `#EXTINF` title and duration rows when metadata is available. Streaming-only saved references are skipped because they need fresh service resolution before playback.

## Maintenance

```sh
rmus scan-local
rmus clear-cache
```

The scan-local command walks configured local music folders and warms the local album-discovery and track-metadata cache. Run it after adding a large local library if the first in-app browse or search feels slow.

The clear-cache command removes cached local album discovery and track metadata. It leaves config, playlists, playback history, queue state, and streaming account tokens untouched.

## Test

```sh
cargo test
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
```

## Configuration

The app stores config, playlists, playback history, the saved queue, and local discovery/track metadata cache under the platform config directory for `com.maximilianpw.rmus`.

Use the in-app settings panel to:

- Add local music folders.
- Edit selected local music folders.
- Remove selected local music folders.
- Configure Qobuz credentials.
- Check Qobuz login.
- Start or refresh Tidal device-code login.
- View Tidal authentication status.
- Clear saved Qobuz/Tidal credentials.
- Cycle maximum streaming quality: MP3, CD, or Hi-Res.
- Adjust startup volume.
- Toggle startup shuffle and cycle startup repeat mode.

## Controls

Global:

- `Tab`: switch focused panel.
- `s`: toggle settings.
- `?`: open keybind help.
- `/`: open search or local filter.
- `Q`: show queue.
- `H`: show recently played tracks.
- `R`: refresh library and playlists.
- `n` / `p`: next/previous track.
- `+` / `-`: adjust volume.
- `m`: mute or restore the previous volume.
- `V`: save the current volume as startup volume.
- `z`: toggle shuffle.
- `r`: cycle repeat mode.
- `Esc`: close the active center/settings view, or quit when there is nothing to close.
- `q` / `Ctrl-C`: quit.

Left panel:

- `h` / `l` or arrow keys: switch source tabs.
- `j` / `k` or arrow keys: move selection.
- `PageUp` / `PageDown`: move selection by 10 rows.
- `Home` / `End`: jump to first/last item.
- `f`: filter the focused Local or Playlists list.
- `Space` / `Enter`: open selected album or playlist.
- `P`: play selected album or playlist.
- `a`: add selected album or playlist to queue.
- `F`: add selected album or playlist to Favorites.
- `U`: remove selected album or playlist from Favorites.
- `C`: create playlist from the Playlists tab.
- `E`: rename selected playlist from the Playlists tab.
- `Y`: duplicate selected playlist from the Playlists tab.
- `D`: delete selected playlist from the Playlists tab.

Center panel:

- `Space` / `Enter`: play from selected song.
- `a`: add selected song to queue.
- `E`: add the open album or playlist to queue.
- `A`: add selected song to playlist.
- `C`: add the open album or playlist to playlist.
- `F`: add selected song or queue track to Favorites.
- `U`: remove selected song or queue track from Favorites.
- `d`: remove selected track from an opened playlist.
- `J` / `K`: move selected track down/up inside an opened playlist.
- `j` / `k` or arrow keys: move selection.
- `PageUp` / `PageDown`: move selection by 10 rows.
- `Home` / `End`: jump to first/last item.
- `Enter`: open selected streaming album or artist result.

Search:

- `Tab`: cycle Albums, Artists, and Tracks search modes.
- Local filters match song title, artist, album, filename, and path while typing.
- All-library local search uses cached metadata plus filename/path fallback; run `rmus scan-local` after adding a large library to prefill richer metadata outside the TUI.
- `Enter`: run the search.
- `Left` / `Right`: move the text cursor.
- `Home` / `End`: jump the text cursor to the start/end.
- `Backspace` / `Delete`: remove text around the cursor.
- `PageUp` / `PageDown`: move result selection by 10 rows.
- `Home` / `End`: jump to first/last result.
- `Esc`: close search.

Right panel:

- `Space`: pause/resume.
- `s`: stop playback.
- `A`: add the current track to a playlist.
- `F`: add the current track to Favorites.
- `U`: remove the current track from Favorites.
- `Left` / `Right`: seek backward/forward.

Queue view:

- `j` / `k` or arrow keys: move queue selection.
- `PageUp` / `PageDown`: move queue selection by 10 rows.
- `Home` / `End`: jump to first/last queue track.
- `f`: filter queue tracks.
- `Space` / `Enter`: jump to selected queue track.
- `A`: add selected queue track to playlist.
- `F`: add selected queue track to Favorites.
- `U`: remove selected queue track from Favorites.
- `J` / `K`: move selected upcoming queue track down/up.
- `S`: save the current queue to a playlist.
- `d`: remove selected queue track.
- `c`: clear queued tracks except the current track.
- `Esc`: close queue view.

History view:

- `j` / `k` or arrow keys: move history selection.
- `PageUp` / `PageDown`: move history selection by 10 rows.
- `Home` / `End`: jump to first/last history track.
- `f`: filter recently played tracks.
- `Space` / `Enter`: play selected history track.
- `d`: remove selected history track.
- `c`: clear recently played history.
- `Esc`: close history view.

Logs panel:

- `j` / `k` or arrow keys: move log selection.
- `PageUp` / `PageDown`: move log selection by 10 rows.
- `Home` / `End`: jump to first/latest log entry.
- `h` / `l` or arrow keys: scroll the selected log message horizontally.
- `c`: clear log history.

Settings panel:

- `Tab` / `l`: next settings tab.
- `Shift+Tab` / `h`: previous settings tab.
- `General: q`: cycle max stream quality.
- `General: +/-`: adjust startup volume.
- `General: z`: toggle startup shuffle.
- `General: r`: cycle startup repeat mode.
- `General: j/k`, `PageUp/PageDown`, `Home/End`: move local source selection.
- `General: a`: add local source.
- `General: e`: edit selected local source.
- `General: d`: remove selected local source.
- `Account: q`: check Qobuz login.
- `Account: t`: start or refresh Tidal login.
- `Account: c`: clear saved streaming accounts.

[cmus]: https://cmus.github.io/
[Ratatui]: https://ratatui.rs

## License

Copyright (c) Maximilian PINDER-WHITE <mpinderwhite@proton.me>

This project is licensed under the MIT license ([LICENSE] or <http://opensource.org/licenses/MIT>).

[LICENSE]: ./LICENSE
