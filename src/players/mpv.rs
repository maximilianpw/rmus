use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::players::{MusicPlayer, PlaybackInfo, PlaybackState, PlayerError, PlayerResult};
use crate::sources::song::Song;

pub struct MpvPlayer {
    process: Option<Child>,
    socket: Option<UnixStream>,
    socket_path: PathBuf,
    message_buffer: String,
    playback_info: PlaybackInfo,
    playlist: Vec<Song>,
    playlist_index: usize,
    request_id: u64,
}

impl Default for MpvPlayer {
    fn default() -> Self {
        Self {
            process: None,
            socket: None,
            socket_path: default_socket_path(),
            message_buffer: String::new(),
            playback_info: PlaybackInfo::default(),
            playlist: Vec::new(),
            playlist_index: 0,
            request_id: 0,
        }
    }
}

impl MpvPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    fn socket_arg(&self) -> String {
        format!("--input-ipc-server={}", self.socket_path.to_string_lossy())
    }

    fn spawn_mpv(&mut self) -> PlayerResult<()> {
        let _ = std::fs::remove_file(&self.socket_path);
        self.message_buffer.clear();

        let socket_arg = self.socket_arg();
        let child = Command::new("mpv")
            .args(["--idle=yes", "--no-video", "--no-terminal"])
            .arg(socket_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PlayerError::ProcessError(e.to_string()))?;

        self.process = Some(child);

        // Wait for socket to be created
        std::thread::sleep(Duration::from_millis(200));

        self.connect_socket()
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
        self.message_buffer.clear();

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
        let mut buf = [0u8; 4096];
        loop {
            let read_result = match self.socket.as_mut() {
                Some(socket) => socket.read(&mut buf),
                None => return Ok(()),
            };

            match read_result {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = String::from_utf8_lossy(&buf[..n]);
                    self.message_buffer.push_str(&chunk);
                    while let Some(idx) = self.message_buffer.find('\n') {
                        let line: String = self.message_buffer.drain(..=idx).collect();
                        let trimmed = line.trim_end_matches(['\n', '\r']);
                        if !trimmed.is_empty() {
                            self.handle_message(trimmed);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(PlayerError::IpcError(e.to_string())),
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, msg: &str) {
        let json: serde_json::Value = match serde_json::from_str(msg) {
            Ok(v) => v,
            Err(_) => return,
        };

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
                    if let Some(paused) = data.and_then(|d| d.as_bool())
                        && self.playback_info.state != PlaybackState::Stopped
                    {
                        self.playback_info.state = if paused {
                            PlaybackState::Paused
                        } else {
                            PlaybackState::Playing
                        };
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

        // Handle end-file event for track completion
        if json.get("event").and_then(|e| e.as_str()) == Some("end-file") {
            let reason = json.get("reason").and_then(|r| r.as_str()).unwrap_or("");
            if reason == "eof" {
                // Auto-advance to next track
                if self.playlist_index + 1 < self.playlist.len() {
                    self.playlist_index += 1;
                    let song = self.playlist[self.playlist_index].clone();
                    self.playback_info.current_song = Some(song.clone());
                    let _ = self.load_file(&song.path);
                    self.playback_info.state = PlaybackState::Playing;
                } else {
                    self.playback_info.state = PlaybackState::Stopped;
                    self.playback_info.current_song = None;
                    self.playback_info.position = 0.0;
                    self.playback_info.duration = 0.0;
                }
            }
        }
    }

    fn load_file(&mut self, path: &Path) -> PlayerResult<()> {
        if !path.is_file() {
            return Err(PlayerError::FileNotFound(
                path.to_string_lossy().into_owned(),
            ));
        }
        let path_str = path.to_string_lossy();
        self.send_command(&["loadfile", &path_str, "replace"])
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
        self.load_file(&song.path)?;
        self.playback_info.state = PlaybackState::Playing;
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
            self.load_file(&song.path)?;
            self.playback_info.state = PlaybackState::Playing;
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
            self.load_file(&song.path)?;
            self.playback_info.state = PlaybackState::Playing;
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
            .field("socket_path", &self.socket_path)
            .finish()
    }
}

fn default_socket_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("rmus-mpv-{}.sock", std::process::id()));
    path
}
