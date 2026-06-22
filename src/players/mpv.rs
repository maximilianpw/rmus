use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::net::UnixStream as IpcStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[cfg(windows)]
type IpcStream = fs::File;

use crate::players::{
    MusicPlayer, PlaybackInfo, PlaybackState, PlayerError, PlayerResult, RepeatMode, ShuffleMode,
};
use crate::sources::song::Song;

pub struct MpvPlayer {
    process: Option<Child>,
    socket: Option<IpcStream>,
    playback_info: PlaybackInfo,
    playlist: Vec<Song>,
    playlist_index: usize,
    request_id: u64,
    socket_path: PathBuf,
    active_temp_stream_path: Option<PathBuf>,
    shuffle: ShuffleMode,
    repeat: RepeatMode,
    /// Mapping from shuffle position → playlist index.
    shuffle_order: Vec<usize>,
    /// Current position within shuffle_order.
    shuffle_position: usize,
}

impl MpvPlayer {
    pub fn new(socket_path: PathBuf) -> Self {
        Self::new_with_default_volume(socket_path, 50)
    }

    pub fn new_with_default_volume(socket_path: PathBuf, default_volume: u16) -> Self {
        Self::new_with_playback_defaults(
            socket_path,
            default_volume,
            ShuffleMode::Off,
            RepeatMode::Off,
        )
    }

    pub fn new_with_playback_defaults(
        socket_path: PathBuf,
        default_volume: u16,
        default_shuffle: ShuffleMode,
        default_repeat: RepeatMode,
    ) -> Self {
        Self {
            process: None,
            socket: None,
            playback_info: PlaybackInfo {
                volume: default_volume.min(100) as u8,
                shuffle: default_shuffle,
                repeat: default_repeat,
                ..Default::default()
            },
            playlist: Vec::new(),
            playlist_index: 0,
            request_id: 0,
            socket_path,
            active_temp_stream_path: None,
            shuffle: default_shuffle,
            repeat: default_repeat,
            shuffle_order: Vec::new(),
            shuffle_position: 0,
        }
    }

    fn build_mpv_args(socket_path: &Path, default_volume: u16) -> Vec<String> {
        let default_volume = default_volume.min(100);
        vec![
            "--idle=yes".to_string(),
            "--no-video".to_string(),
            "--audio-display=no".to_string(),
            "--cover-art-auto=no".to_string(),
            format!("--volume={default_volume}"),
            "--demuxer-lavf-o-add=protocol_whitelist=[file,crypto,data,http,https,tcp,tls]"
                .to_string(),
            "--no-terminal".to_string(),
            format!("--input-ipc-server={}", Self::ipc_endpoint(socket_path)),
        ]
    }

    fn ipc_endpoint(socket_path: &Path) -> String {
        #[cfg(windows)]
        {
            let pipe_name: String = socket_path
                .to_string_lossy()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let pipe_name = if pipe_name.is_empty() {
                "rmus_mpv".to_string()
            } else {
                pipe_name
            };
            format!(r"\\.\pipe\{pipe_name}")
        }

        #[cfg(not(windows))]
        {
            socket_path.to_string_lossy().into_owned()
        }
    }

    fn spawn_mpv(&mut self) -> PlayerResult<()> {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket_path);

        let child = Command::new("mpv")
            .args(Self::build_mpv_args(
                &self.socket_path,
                self.playback_info.volume as u16,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PlayerError::ProcessError(e.to_string()))?;

        self.process = Some(child);

        // Poll for the IPC endpoint to accept connections (up to 3 seconds).
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            #[cfg(unix)]
            if self.socket_path.exists() {
                match self.connect_socket() {
                    Ok(()) => return Ok(()),
                    Err(_) => {
                        // Socket file exists but not ready yet
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }

            #[cfg(windows)]
            match self.connect_socket() {
                Ok(()) => return Ok(()),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }

        Err(PlayerError::IpcError(format!(
            "Timed out waiting for mpv IPC at {}",
            Self::ipc_endpoint(&self.socket_path)
        )))
    }

    fn connect_socket(&mut self) -> PlayerResult<()> {
        #[cfg(unix)]
        let stream = {
            let stream = IpcStream::connect(&self.socket_path)
                .map_err(|e| PlayerError::IpcError(format!("Failed to connect: {}", e)))?;

            stream
                .set_read_timeout(Some(Duration::from_millis(10)))
                .ok();
            stream
                .set_write_timeout(Some(Duration::from_millis(50)))
                .ok();
            stream.set_nonblocking(true).ok();
            stream
        };

        #[cfg(windows)]
        let stream = {
            let endpoint = Self::ipc_endpoint(&self.socket_path);
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&endpoint)
                .map_err(|e| {
                    PlayerError::IpcError(format!("Failed to connect to {endpoint}: {e}"))
                })?
        };

        self.socket = Some(stream);

        // Observe properties for real-time updates
        self.observe_property("time-pos", 1)?;
        self.observe_property("duration", 2)?;
        self.observe_property("pause", 3)?;
        self.observe_property("volume", 4)?;
        self.observe_property("eof-reached", 5)?;

        Ok(())
    }

    fn send_command(&mut self, command: &[&str]) -> PlayerResult<()> {
        let socket = self.socket.as_mut().ok_or(PlayerError::NotConnected)?;

        self.request_id += 1;
        let cmd = serde_json::json!({
            "command": command,
            "request_id": self.request_id
        });

        let mut msg = cmd.to_string();
        msg.push('\n');

        socket
            .write_all(msg.as_bytes())
            .map_err(|e| PlayerError::IpcError(e.to_string()))?;
        socket
            .flush()
            .map_err(|e| PlayerError::IpcError(e.to_string()))?;

        Ok(())
    }

    fn observe_property(&mut self, property: &str, id: u64) -> PlayerResult<()> {
        let socket = self.socket.as_mut().ok_or(PlayerError::NotConnected)?;

        let cmd = serde_json::json!({
            "command": ["observe_property", id, property]
        });

        let mut msg = cmd.to_string();
        msg.push('\n');

        socket
            .write_all(msg.as_bytes())
            .map_err(|e| PlayerError::IpcError(e.to_string()))?;

        Ok(())
    }

    fn process_messages(&mut self) -> PlayerResult<()> {
        #[cfg(windows)]
        {
            return Ok(());
        }

        #[cfg(unix)]
        {
            let socket = match self.socket.as_mut() {
                Some(s) => s,
                None => return Ok(()),
            };

            let socket_clone = match socket.try_clone() {
                Ok(s) => s,
                Err(_) => return Ok(()),
            };

            let mut reader = BufReader::new(socket_clone);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        self.handle_message(&line);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }

            Ok(())
        }
    }

    fn handle_message(&mut self, msg: &str) {
        let json: serde_json::Value = match serde_json::from_str(msg) {
            Ok(v) => v,
            Err(_) => return,
        };

        match json.get("event").and_then(|e| e.as_str()) {
            Some("file-loaded" | "playback-restart") => {
                if self.playback_info.current_song.is_some() {
                    self.playback_info.last_error = None;
                    self.playback_info.state = PlaybackState::Playing;
                }
            }
            Some("end-file") => {
                let reason = json.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                match reason {
                    "eof" => {
                        if !self.advance_to_next_track() {
                            self.reset_playback_state();
                        }
                    }
                    "stop" | "quit" | "error" => {
                        if reason == "error" {
                            let msg = json
                                .get("file_error")
                                .and_then(|e| e.as_str())
                                .unwrap_or("unknown playback failure");
                            self.playback_info.last_error = Some(msg.to_string());
                        }
                        self.reset_playback_state();
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        // Handle property change events
        if json.get("event").and_then(|e| e.as_str()) == Some("property-change") {
            let name = json.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let data = json.get("data");

            match name {
                "time-pos" => {
                    if let Some(pos) = data.and_then(|d| d.as_f64()) {
                        self.playback_info.position = pos;
                    }
                }
                "duration" => {
                    if let Some(dur) = data.and_then(|d| d.as_f64()) {
                        self.playback_info.duration = dur;
                    }
                }
                "pause" => {
                    if let Some(paused) = data.and_then(|d| d.as_bool()) {
                        if self.playback_info.state != PlaybackState::Stopped {
                            self.playback_info.state = if paused {
                                PlaybackState::Paused
                            } else {
                                PlaybackState::Playing
                            };
                        }
                    }
                }
                "volume" => {
                    if let Some(vol) = data.and_then(|d| d.as_f64()) {
                        self.playback_info.volume = vol as u8;
                    }
                }
                "eof-reached" => {
                    if data.and_then(|d| d.as_bool()) == Some(true) {
                        // Track ended - will be handled by end-file event
                    }
                }
                _ => {}
            }
        }
    }

    fn generate_shuffle_order(&mut self, current_first: bool) {
        use rand::seq::SliceRandom;
        let len = self.playlist.len();
        if len == 0 {
            self.shuffle_order.clear();
            self.shuffle_position = 0;
            return;
        }
        let mut indices: Vec<usize> = (0..len).collect();
        let mut rng = rand::rng();
        indices.shuffle(&mut rng);

        if current_first {
            // Put the current playlist_index at position 0 so the current song stays put.
            if let Some(pos) = indices.iter().position(|&i| i == self.playlist_index) {
                indices.swap(0, pos);
            }
            self.shuffle_position = 0;
        } else {
            self.shuffle_position = 0;
        }
        self.shuffle_order = indices;
    }

    /// Advance to the next track respecting shuffle/repeat. Returns true if a new track started.
    fn advance_to_next_track(&mut self) -> bool {
        if self.playlist.is_empty() {
            return false;
        }

        if self.repeat == RepeatMode::One {
            // Reload the current song
            let song = self.playlist[self.playlist_index].clone();
            self.playback_info.current_song = Some(song.clone());
            self.playback_info.position = 0.0;
            self.playback_info.last_error = None;
            let _ = self.load_song(&song);
            return true;
        }

        let next = if self.shuffle == ShuffleMode::On {
            if self.shuffle_position + 1 < self.shuffle_order.len() {
                self.shuffle_position += 1;
                Some(self.shuffle_order[self.shuffle_position])
            } else if self.repeat == RepeatMode::All {
                self.generate_shuffle_order(false);
                Some(self.shuffle_order[0])
            } else {
                None
            }
        } else if self.playlist_index + 1 < self.playlist.len() {
            Some(self.playlist_index + 1)
        } else if self.repeat == RepeatMode::All {
            Some(0)
        } else {
            None
        };

        match next {
            Some(idx) => {
                self.playlist_index = idx;
                let song = self.playlist[idx].clone();
                self.playback_info.current_song = Some(song.clone());
                self.playback_info.position = 0.0;
                self.playback_info.last_error = None;
                let _ = self.load_song(&song);
                true
            }
            None => false,
        }
    }

    /// Go to the previous track respecting shuffle/repeat.
    fn go_to_previous_track(&mut self) -> bool {
        if self.playlist.is_empty() {
            return false;
        }

        let prev = if self.shuffle == ShuffleMode::On {
            if self.shuffle_position > 0 {
                self.shuffle_position -= 1;
                Some(self.shuffle_order[self.shuffle_position])
            } else if self.repeat == RepeatMode::All {
                self.shuffle_position = self.shuffle_order.len().saturating_sub(1);
                Some(self.shuffle_order[self.shuffle_position])
            } else {
                None // At start, will just restart current
            }
        } else if self.playlist_index > 0 {
            Some(self.playlist_index - 1)
        } else if self.repeat == RepeatMode::All {
            Some(self.playlist.len() - 1)
        } else {
            None
        };

        match prev {
            Some(idx) => {
                self.playlist_index = idx;
                let song = self.playlist[idx].clone();
                self.playback_info.current_song = Some(song.clone());
                self.playback_info.position = 0.0;
                self.playback_info.last_error = None;
                let _ = self.load_song(&song);
                true
            }
            None => {
                // At start, just restart current track
                let _ = self.seek_internal(0.0);
                false
            }
        }
    }

    fn seek_internal(&mut self, position: f64) -> PlayerResult<()> {
        self.send_command(&["seek", &position.to_string(), "absolute"])
    }

    fn load_song(&mut self, song: &Song) -> PlayerResult<()> {
        let source = if let Some(ref url) = song.url {
            self.cleanup_temp_stream_file();
            url.clone()
        } else if let Some(ref manifest) = song.stream_manifest {
            self.materialize_stream_manifest(manifest)?
        } else {
            self.cleanup_temp_stream_file();
            song.path.to_string_lossy().into_owned()
        };
        self.send_command(&["loadfile", &source, "replace"])
    }

    fn reset_playback_state(&mut self) {
        self.cleanup_temp_stream_file();
        self.playback_info.state = PlaybackState::Stopped;
        self.playback_info.current_song = None;
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
    }

    fn materialize_stream_manifest(
        &mut self,
        manifest: &crate::sources::song::StreamManifest,
    ) -> PlayerResult<String> {
        self.cleanup_temp_stream_file();

        let parent = self.socket_path.parent().ok_or_else(|| {
            PlayerError::ProcessError("Missing mpv socket parent directory".to_string())
        })?;
        fs::create_dir_all(parent).map_err(|e| PlayerError::ProcessError(e.to_string()))?;

        let filename = format!(
            "stream-{}.{}",
            self.request_id.saturating_add(1),
            manifest.file_extension
        );
        let path = parent.join(filename);
        fs::write(&path, &manifest.contents)
            .map_err(|e| PlayerError::ProcessError(e.to_string()))?;
        self.active_temp_stream_path = Some(path.clone());

        Ok(path.to_string_lossy().into_owned())
    }

    fn cleanup_temp_stream_file(&mut self) {
        if let Some(path) = self.active_temp_stream_path.take() {
            let _ = fs::remove_file(path);
        }
    }

    fn ensure_running(&mut self) -> PlayerResult<()> {
        let needs_spawn = match &mut self.process {
            Some(child) => match child.try_wait() {
                Ok(Some(_)) => true, // Process exited
                Ok(None) => false,   // Still running
                Err(_) => true,      // Error checking, respawn
            },
            None => true,
        };

        if needs_spawn {
            self.spawn_mpv()?;
        }

        Ok(())
    }
}

impl MusicPlayer for MpvPlayer {
    fn play(&mut self, song: &Song) -> PlayerResult<()> {
        self.ensure_running()?;
        self.playlist = vec![song.clone()];
        self.playlist_index = 0;
        self.playback_info.current_song = Some(song.clone());
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
        self.playback_info.last_error = None;
        self.load_song(song)?;
        self.playback_info.state = PlaybackState::Stopped;
        Ok(())
    }

    fn play_album(&mut self, songs: Vec<Song>, start_index: usize) -> PlayerResult<()> {
        if songs.is_empty() {
            return Ok(());
        }
        self.ensure_running()?;
        self.playlist = songs;
        self.playlist_index = start_index.min(self.playlist.len() - 1);
        if self.shuffle == ShuffleMode::On {
            self.generate_shuffle_order(true);
        }
        let song = self.playlist[self.playlist_index].clone();
        self.playback_info.current_song = Some(song.clone());
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
        self.playback_info.last_error = None;
        self.load_song(&song)?;
        self.playback_info.state = PlaybackState::Stopped;
        Ok(())
    }

    fn toggle_pause(&mut self) -> PlayerResult<()> {
        if self.playback_info.state == PlaybackState::Stopped {
            return Ok(());
        }
        self.send_command(&["cycle", "pause"])
    }

    fn stop(&mut self) -> PlayerResult<()> {
        self.send_command(&["stop"])?;
        self.reset_playback_state();
        Ok(())
    }

    fn next(&mut self) -> PlayerResult<()> {
        self.advance_to_next_track();
        Ok(())
    }

    fn previous(&mut self) -> PlayerResult<()> {
        // If more than 3 seconds in, restart current track
        if self.playback_info.position > 3.0 {
            self.seek(0.0)?;
        } else {
            self.go_to_previous_track();
        }
        Ok(())
    }

    fn seek(&mut self, position: f64) -> PlayerResult<()> {
        self.seek_internal(position)
    }

    fn set_volume(&mut self, volume: u8) -> PlayerResult<()> {
        let vol = volume.min(100);
        self.send_command(&["set_property", "volume", &vol.to_string()])
    }

    fn poll(&mut self) -> PlayerResult<PlaybackInfo> {
        if self.socket.is_some() {
            self.process_messages()?;
        }
        self.playback_info.shuffle = self.shuffle;
        self.playback_info.repeat = self.repeat;
        Ok(self.playback_info.clone())
    }

    fn get_playback_info(&self) -> &PlaybackInfo {
        &self.playback_info
    }

    fn is_alive(&self) -> bool {
        match &self.process {
            Some(_) => self.socket.is_some(),
            None => false,
        }
    }

    fn enqueue(&mut self, songs: Vec<Song>) -> PlayerResult<()> {
        if songs.is_empty() {
            return Ok(());
        }

        let was_empty = self.playlist.is_empty();
        let first_new_index = self.playlist.len();
        self.playlist.extend(songs);

        // If shuffle is on, insert new indices into remaining shuffle positions
        if self.shuffle == ShuffleMode::On && !self.shuffle_order.is_empty() {
            use rand::Rng;
            let mut rng = rand::rng();
            for idx in first_new_index..self.playlist.len() {
                // Insert at a random position after the current shuffle_position
                let insert_at = if self.shuffle_position + 1 < self.shuffle_order.len() {
                    rng.random_range((self.shuffle_position + 1)..=self.shuffle_order.len())
                } else {
                    self.shuffle_order.len()
                };
                self.shuffle_order.insert(insert_at, idx);
            }
        }

        // If nothing was playing, start playing the first enqueued song
        if was_empty {
            self.ensure_running()?;
            self.playlist_index = first_new_index;
            if self.shuffle == ShuffleMode::On {
                self.generate_shuffle_order(true);
            }
            let song = self.playlist[self.playlist_index].clone();
            self.playback_info.current_song = Some(song.clone());
            self.playback_info.position = 0.0;
            self.playback_info.duration = 0.0;
            self.playback_info.last_error = None;
            self.load_song(&song)?;
            self.playback_info.state = PlaybackState::Stopped;
        }

        Ok(())
    }

    fn get_queue(&self) -> &[Song] {
        &self.playlist
    }

    fn get_queue_position(&self) -> usize {
        self.playlist_index
    }

    fn remove_from_queue(&mut self, index: usize) -> PlayerResult<()> {
        if index >= self.playlist.len() {
            return Err(PlayerError::ValidationError(format!(
                "Queue index {} out of bounds for {} songs",
                index,
                self.playlist.len()
            )));
        }

        // Don't allow removing the currently playing track
        if index == self.playlist_index {
            return Err(PlayerError::ValidationError(
                "Cannot remove the currently playing track".to_string(),
            ));
        }

        self.playlist.remove(index);

        // Adjust playlist_index if the removed item was before the current
        if index < self.playlist_index {
            self.playlist_index -= 1;
        }

        // Rebuild shuffle order if shuffling
        if self.shuffle == ShuffleMode::On {
            self.generate_shuffle_order(true);
        }

        Ok(())
    }

    fn move_in_queue(&mut self, from: usize, to: usize) -> PlayerResult<()> {
        if from >= self.playlist.len() || to >= self.playlist.len() {
            return Err(PlayerError::ValidationError(format!(
                "Queue move {} -> {} out of bounds for {} songs",
                from,
                to,
                self.playlist.len()
            )));
        }

        if from == self.playlist_index || to == self.playlist_index {
            return Err(PlayerError::ValidationError(
                "Cannot move the currently playing track".to_string(),
            ));
        }

        if from == to {
            return Ok(());
        }

        let song = self.playlist.remove(from);
        self.playlist.insert(to, song);

        if self.shuffle == ShuffleMode::On {
            self.generate_shuffle_order(true);
        }

        Ok(())
    }

    fn toggle_shuffle(&mut self) -> PlayerResult<()> {
        self.shuffle = match self.shuffle {
            ShuffleMode::Off => {
                self.generate_shuffle_order(true);
                ShuffleMode::On
            }
            ShuffleMode::On => {
                self.shuffle_order.clear();
                self.shuffle_position = 0;
                ShuffleMode::Off
            }
        };
        self.playback_info.shuffle = self.shuffle;
        Ok(())
    }

    fn cycle_repeat(&mut self) -> PlayerResult<()> {
        self.repeat = self.repeat.cycle();
        self.playback_info.repeat = self.repeat;
        Ok(())
    }

    fn shutdown(&mut self) -> PlayerResult<()> {
        if let Some(ref mut socket) = self.socket {
            let cmd = serde_json::json!({"command": ["quit"]});
            let mut msg = cmd.to_string();
            msg.push('\n');
            let _ = socket.write_all(msg.as_bytes());
        }

        if let Some(mut process) = self.process.take() {
            let _ = process.wait();
        }

        self.socket = None;
        self.cleanup_temp_stream_file();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.socket_path);

        Ok(())
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

impl std::fmt::Debug for MpvPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MpvPlayer")
            .field("playback_info", &self.playback_info)
            .field("playlist_len", &self.playlist.len())
            .field("playlist_index", &self.playlist_index)
            .field("is_connected", &self.socket.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{
        players::{MusicPlayer, PlaybackState, RepeatMode, ShuffleMode},
        sources::song::Song,
    };

    use super::MpvPlayer;

    #[test]
    fn mpv_is_launched_in_audio_only_mode() {
        let args = MpvPlayer::build_mpv_args(Path::new("/tmp/rmus-mpv.sock"), 73);

        assert!(args.iter().any(|arg| arg == "--no-video"));
        assert!(args.iter().any(|arg| arg == "--audio-display=no"));
        assert!(args.iter().any(|arg| arg == "--cover-art-auto=no"));
        assert!(args.iter().any(|arg| arg == "--volume=73"));
        assert!(args.iter().any(|arg| {
            arg == "--demuxer-lavf-o-add=protocol_whitelist=[file,crypto,data,http,https,tcp,tls]"
        }));

        #[cfg(unix)]
        assert!(args
            .iter()
            .any(|arg| arg == "--input-ipc-server=/tmp/rmus-mpv.sock"));

        #[cfg(windows)]
        assert!(args
            .iter()
            .any(|arg| arg == r"--input-ipc-server=\\.\pipe\_tmp_rmus_mpv_sock"));
    }

    #[test]
    fn configured_startup_volume_is_clamped_for_mpv() {
        let args = MpvPlayer::build_mpv_args(Path::new("/tmp/rmus-mpv.sock"), 175);

        assert!(args.iter().any(|arg| arg == "--volume=100"));
    }

    #[test]
    fn configured_playback_defaults_are_exposed_before_polling() {
        let player = MpvPlayer::new_with_playback_defaults(
            "/tmp/rmus.sock".into(),
            35,
            ShuffleMode::On,
            RepeatMode::All,
        );

        let info = player.get_playback_info();
        assert_eq!(info.volume, 35);
        assert_eq!(info.shuffle, ShuffleMode::On);
        assert_eq!(info.repeat, RepeatMode::All);
    }

    #[test]
    fn file_loaded_event_marks_playback_as_playing() {
        let mut player = MpvPlayer::new("/tmp/rmus.sock".into());
        player.playback_info.current_song = Some(Song::from_url(
            "Track".to_string(),
            "https://example.com/track.flac".to_string(),
            Some("Hi-Res".to_string()),
        ));

        player.handle_message(r#"{"event":"file-loaded"}"#);

        assert_eq!(player.playback_info.state, PlaybackState::Playing);
    }

    #[test]
    fn end_file_error_clears_false_playing_state() {
        let mut player = MpvPlayer::new("/tmp/rmus.sock".into());
        player.playback_info.current_song = Some(Song::from_url(
            "Track".to_string(),
            "https://example.com/track.flac".to_string(),
            Some("Hi-Res".to_string()),
        ));
        player.playback_info.state = PlaybackState::Playing;
        player.playback_info.position = 12.0;
        player.playback_info.duration = 99.0;

        player.handle_message(
            r#"{"event":"end-file","reason":"error","file_error":"loading failed"}"#,
        );

        assert_eq!(player.playback_info.state, PlaybackState::Stopped);
        assert!(player.playback_info.current_song.is_none());
        assert_eq!(player.playback_info.position, 0.0);
        assert_eq!(player.playback_info.duration, 0.0);
        assert_eq!(
            player.playback_info.last_error.as_deref(),
            Some("loading failed")
        );
    }

    #[test]
    fn queue_move_reorders_upcoming_tracks_without_moving_current() {
        let mut player = MpvPlayer::new("/tmp/rmus.sock".into());
        player.playlist = vec![
            Song {
                title: "Current".to_string(),
                ..Default::default()
            },
            Song {
                title: "Second".to_string(),
                ..Default::default()
            },
            Song {
                title: "Third".to_string(),
                ..Default::default()
            },
        ];
        player.playlist_index = 0;

        player.move_in_queue(1, 2).unwrap();

        let titles: Vec<_> = player
            .playlist
            .iter()
            .map(|song| song.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Current", "Third", "Second"]);
        assert_eq!(player.playlist_index, 0);

        let err = player.move_in_queue(1, 0).unwrap_err();
        assert!(
            err.to_string()
                .contains("Cannot move the currently playing track"),
            "moving another item into the current slot should be rejected"
        );
    }
}
