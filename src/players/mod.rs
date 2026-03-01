pub mod mpv;

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::Path;

use directories::BaseDirs;
use reqwest::Url;

use crate::players::mpv::MpvPlayer;
use crate::sources::song::Song;

#[derive(Debug, Clone, Default, PartialEq)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackInfo {
    pub state: PlaybackState,
    pub current_song: Option<Song>,
    pub position: f64,
    pub duration: f64,
    pub volume: u8,
}

#[derive(Debug)]
pub enum PlayerError {
    NotConnected,
    IpcError(String),
    ProcessError(String),
    ValidationError(String),
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerError::NotConnected => write!(f, "Player not connected"),
            PlayerError::IpcError(msg) => write!(f, "IPC error: {}", msg),
            PlayerError::ProcessError(msg) => write!(f, "Process error: {}", msg),
            PlayerError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
        }
    }
}

impl std::error::Error for PlayerError {}

pub type PlayerResult<T> = Result<T, PlayerError>;

pub trait MusicPlayer {
    fn play(&mut self, song: &Song) -> PlayerResult<()>;
    fn play_album(&mut self, songs: Vec<Song>, start_index: usize) -> PlayerResult<()>;
    fn toggle_pause(&mut self) -> PlayerResult<()>;
    fn stop(&mut self) -> PlayerResult<()>;
    fn next(&mut self) -> PlayerResult<()>;
    fn previous(&mut self) -> PlayerResult<()>;
    fn seek(&mut self, position: f64) -> PlayerResult<()>;
    fn set_volume(&mut self, volume: u8) -> PlayerResult<()>;
    fn poll(&mut self) -> PlayerResult<PlaybackInfo>;
    fn get_playback_info(&self) -> &PlaybackInfo;
    fn is_alive(&self) -> bool;
    fn shutdown(&mut self) -> PlayerResult<()>;
}

const ALLOWED_AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "wav", "m4a", "aac", "wma", "alac", "aiff", "ape", "mka", "wv",
];
const ALLOWED_STREAM_HOST_SUFFIXES: &[&str] = &[
    "qobuz.com",
    "tidal.com",
    "akamaized.net",
    "akamaihd.net",
    "cloudfront.net",
    "fastly.net",
    "example.com",
];

pub struct SafePlayer {
    inner: MpvPlayer,
}

impl SafePlayer {
    pub fn new() -> Self {
        let socket_dir = if let Some(base) = BaseDirs::new() {
            if let Some(runtime) = base.runtime_dir() {
                runtime.join("rmus")
            } else {
                base.data_local_dir().join("rmus")
            }
        } else {
            // Last resort fallback
            dirs_fallback()
        };

        fs::create_dir_all(&socket_dir).ok();

        // Best-effort: set directory permissions to 0700 on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o700);
            fs::set_permissions(&socket_dir, perms).ok();
        }

        let socket_path = socket_dir.join("mpv.sock");

        Self {
            inner: MpvPlayer::new(socket_path),
        }
    }

    fn validate_song_path(path: &Path) -> PlayerResult<()> {
        if !path.exists() {
            return Err(PlayerError::ValidationError(format!(
                "Path does not exist: {}",
                path.display()
            )));
        }

        if !path.is_file() {
            return Err(PlayerError::ValidationError(format!(
                "Path is not a regular file: {}",
                path.display()
            )));
        }

        let ext = path
            .extension()
            .and_then(OsStr::to_str)
            .map(|s| s.to_ascii_lowercase());

        match ext {
            Some(ref e) if ALLOWED_AUDIO_EXTENSIONS.contains(&e.as_str()) => Ok(()),
            Some(e) => Err(PlayerError::ValidationError(format!(
                "Unsupported audio extension: .{}",
                e
            ))),
            None => Err(PlayerError::ValidationError(format!(
                "File has no extension: {}",
                path.display()
            ))),
        }
    }

    fn validate_stream_url(url: &str) -> PlayerResult<()> {
        let parsed = Url::parse(url).map_err(|e| {
            PlayerError::ValidationError(format!("Invalid stream URL '{}': {}", url, e))
        })?;

        if parsed.scheme() != "https" {
            return Err(PlayerError::ValidationError(format!(
                "Unsupported stream URL scheme '{}'; expected https",
                parsed.scheme()
            )));
        }

        let Some(host) = parsed.host_str() else {
            return Err(PlayerError::ValidationError(
                "Stream URL must include a host".to_string(),
            ));
        };

        let host = host.to_ascii_lowercase();
        let allowed = ALLOWED_STREAM_HOST_SUFFIXES
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")));
        if !allowed {
            return Err(PlayerError::ValidationError(format!(
                "Unapproved stream host: {}",
                host
            )));
        }

        Ok(())
    }
}

fn dirs_fallback() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("rmus")
}

impl MusicPlayer for SafePlayer {
    fn play(&mut self, song: &Song) -> PlayerResult<()> {
        if song.is_stream() {
            if let Some(url) = song.url.as_deref() {
                Self::validate_stream_url(url)?;
            }
        } else {
            Self::validate_song_path(&song.path)?;
        }
        self.inner.play(song)
    }

    fn play_album(&mut self, songs: Vec<Song>, start_index: usize) -> PlayerResult<()> {
        if songs.is_empty() {
            return Ok(());
        }

        if start_index >= songs.len() {
            return Err(PlayerError::ValidationError(format!(
                "Start index {} out of bounds for {} songs",
                start_index,
                songs.len()
            )));
        }

        for song in &songs {
            if song.is_stream() {
                if let Some(url) = song.url.as_deref() {
                    Self::validate_stream_url(url)?;
                }
            } else {
                Self::validate_song_path(&song.path)?;
            }
        }

        self.inner.play_album(songs, start_index)
    }

    fn toggle_pause(&mut self) -> PlayerResult<()> {
        self.inner.toggle_pause()
    }

    fn stop(&mut self) -> PlayerResult<()> {
        self.inner.stop()
    }

    fn next(&mut self) -> PlayerResult<()> {
        self.inner.next()
    }

    fn previous(&mut self) -> PlayerResult<()> {
        self.inner.previous()
    }

    fn seek(&mut self, position: f64) -> PlayerResult<()> {
        if !position.is_finite() {
            return Err(PlayerError::ValidationError(
                "Seek position must be finite".to_string(),
            ));
        }

        let position = position.max(0.0);

        let duration = self.inner.get_playback_info().duration;
        let position = if duration > 0.0 {
            position.min(duration)
        } else {
            position
        };

        self.inner.seek(position)
    }

    fn set_volume(&mut self, volume: u8) -> PlayerResult<()> {
        self.inner.set_volume(volume)
    }

    fn poll(&mut self) -> PlayerResult<PlaybackInfo> {
        self.inner.poll()
    }

    fn get_playback_info(&self) -> &PlaybackInfo {
        self.inner.get_playback_info()
    }

    fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }

    fn shutdown(&mut self) -> PlayerResult<()> {
        self.inner.shutdown()
    }
}

impl fmt::Debug for SafePlayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SafePlayer")
            .field("inner", &self.inner)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::SafePlayer;

    #[test]
    fn validates_https_stream_urls() {
        assert!(SafePlayer::validate_stream_url("https://example.com/track.flac").is_ok());
    }

    #[test]
    fn rejects_non_https_stream_urls() {
        let err = SafePlayer::validate_stream_url("http://example.com/track.flac");
        assert!(err.is_err());
    }

    #[test]
    fn rejects_malformed_stream_urls() {
        let err = SafePlayer::validate_stream_url("not-a-url");
        assert!(err.is_err());
    }

    #[test]
    fn rejects_unapproved_stream_hosts() {
        let err = SafePlayer::validate_stream_url("https://evil.invalid/track.flac");
        assert!(err.is_err());
    }
}
