use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::players::{MusicPlayer, PlaybackInfo, PlaybackState, PlayerError, PlayerResult};
use crate::sources::song::Song;

pub struct MpvPlayer {
    process: Option<Child>,
    socket: Option<UnixStream>,
    playback_info: PlaybackInfo,
    playlist: Vec<Song>,
    playlist_index: usize,
    request_id: u64,
    socket_path: PathBuf,
}

impl MpvPlayer {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            process: None,
            socket: None,
            playback_info: PlaybackInfo::default(),
            playlist: Vec::new(),
            playlist_index: 0,
            request_id: 0,
            socket_path,
        }
    }

    fn build_mpv_args(socket_path: &std::path::Path) -> Vec<String> {
        vec![
            "--idle=yes".to_string(),
            "--no-video".to_string(),
            "--audio-display=no".to_string(),
            "--cover-art-auto=no".to_string(),
            "--no-terminal".to_string(),
            format!("--input-ipc-server={}", socket_path.display()),
        ]
    }

    fn spawn_mpv(&mut self) -> PlayerResult<()> {
        let _ = std::fs::remove_file(&self.socket_path);

        let child = Command::new("mpv")
            .args(Self::build_mpv_args(&self.socket_path))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PlayerError::ProcessError(e.to_string()))?;

        self.process = Some(child);

        // Poll for socket file to appear (up to 3 seconds)
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
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
        }

        Err(PlayerError::IpcError(format!(
            "Timed out waiting for mpv socket at {}",
            self.socket_path.display()
        )))
    }

    fn connect_socket(&mut self) -> PlayerResult<()> {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| PlayerError::IpcError(format!("Failed to connect: {}", e)))?;

        stream
            .set_read_timeout(Some(Duration::from_millis(10)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_millis(50)))
            .ok();
        stream.set_nonblocking(true).ok();

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

    fn handle_message(&mut self, msg: &str) {
        let json: serde_json::Value = match serde_json::from_str(msg) {
            Ok(v) => v,
            Err(_) => return,
        };

        match json.get("event").and_then(|e| e.as_str()) {
            Some("file-loaded" | "playback-restart") => {
                if self.playback_info.current_song.is_some() {
                    self.playback_info.state = PlaybackState::Playing;
                }
            }
            Some("end-file") => {
                let reason = json.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                match reason {
                    "eof" => {
                        if self.playlist_index + 1 < self.playlist.len() {
                            self.playlist_index += 1;
                            let song = self.playlist[self.playlist_index].clone();
                            self.playback_info.current_song = Some(song.clone());
                            let _ = self.load_song(&song);
                        } else {
                            self.reset_playback_state();
                        }
                    }
                    "stop" | "quit" | "error" => {
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

    fn load_song(&mut self, song: &Song) -> PlayerResult<()> {
        let source = if let Some(ref url) = song.url {
            url.clone()
        } else {
            song.path.to_string_lossy().into_owned()
        };
        self.send_command(&["loadfile", &source, "replace"])
    }

    fn reset_playback_state(&mut self) {
        self.playback_info.state = PlaybackState::Stopped;
        self.playback_info.current_song = None;
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
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
        let song = self.playlist[self.playlist_index].clone();
        self.playback_info.current_song = Some(song.clone());
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
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
        self.playback_info.state = PlaybackState::Stopped;
        self.playback_info.current_song = None;
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
        Ok(())
    }

    fn next(&mut self) -> PlayerResult<()> {
        if self.playlist_index + 1 < self.playlist.len() {
            self.playlist_index += 1;
            let song = self.playlist[self.playlist_index].clone();
            self.playback_info.current_song = Some(song.clone());
            self.playback_info.position = 0.0;
            self.load_song(&song)?;
            self.playback_info.state = PlaybackState::Stopped;
        }
        Ok(())
    }

    fn previous(&mut self) -> PlayerResult<()> {
        // If more than 3 seconds in, restart current track
        if self.playback_info.position > 3.0 {
            self.seek(0.0)?;
        } else if self.playlist_index > 0 {
            self.playlist_index -= 1;
            let song = self.playlist[self.playlist_index].clone();
            self.playback_info.current_song = Some(song.clone());
            self.playback_info.position = 0.0;
            self.load_song(&song)?;
            self.playback_info.state = PlaybackState::Stopped;
        } else {
            // At first track, just restart it
            self.seek(0.0)?;
        }
        Ok(())
    }

    fn seek(&mut self, position: f64) -> PlayerResult<()> {
        self.send_command(&["seek", &position.to_string(), "absolute"])
    }

    fn set_volume(&mut self, volume: u8) -> PlayerResult<()> {
        let vol = volume.min(100);
        self.send_command(&["set_property", "volume", &vol.to_string()])
    }

    fn poll(&mut self) -> PlayerResult<PlaybackInfo> {
        if self.socket.is_some() {
            self.process_messages()?;
        }
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

    use crate::{players::PlaybackState, sources::song::Song};

    use super::MpvPlayer;

    #[test]
    fn mpv_is_launched_in_audio_only_mode() {
        let args = MpvPlayer::build_mpv_args(Path::new("/tmp/rmus-mpv.sock"));

        assert!(args.iter().any(|arg| arg == "--no-video"));
        assert!(args.iter().any(|arg| arg == "--audio-display=no"));
        assert!(args.iter().any(|arg| arg == "--cover-art-auto=no"));
        assert!(args
            .iter()
            .any(|arg| arg == "--input-ipc-server=/tmp/rmus-mpv.sock"));
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
    }
}
