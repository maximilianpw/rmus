use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::players::{MusicPlayer, PlaybackInfo, PlaybackState, PlayerError, PlayerResult};
use crate::sources::song::Song;

const SOCKET_PATH: &str = "/tmp/rmus-mpv.sock";

#[derive(Default)]
pub struct MpvPlayer {
    process: Option<Child>,
    socket: Option<UnixStream>,
    playback_info: PlaybackInfo,
    playlist: Vec<Song>,
    playlist_index: usize,
    request_id: u64,
}

impl MpvPlayer {
    pub fn new() -> Self {
        Self::default()
    }

    fn spawn_mpv(&mut self) -> PlayerResult<()> {
        let _ = std::fs::remove_file(SOCKET_PATH);

        let child = Command::new("mpv")
            .args([
                "--idle=yes",
                "--no-video",
                "--no-terminal",
                &format!("--input-ipc-server={}", SOCKET_PATH),
            ])
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
        let stream = UnixStream::connect(SOCKET_PATH)
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

    fn load_file(&mut self, path: &PathBuf) -> PlayerResult<()> {
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
    fn play(&mut self, song: &Song) -> PlayerResult<()> {
        self.ensure_running()?;
        self.playlist = vec![song.clone()];
        self.playlist_index = 0;
        self.playback_info.current_song = Some(song.clone());
        self.playback_info.position = 0.0;
        self.playback_info.duration = 0.0;
        self.load_file(&song.path)?;
        self.playback_info.state = PlaybackState::Playing;
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
        let _ = std::fs::remove_file(SOCKET_PATH);

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
