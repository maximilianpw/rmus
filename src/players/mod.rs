pub mod mpv;

use std::fmt;

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
}

impl fmt::Display for PlayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlayerError::NotConnected => write!(f, "Player not connected"),
            PlayerError::IpcError(msg) => write!(f, "IPC error: {}", msg),
            PlayerError::ProcessError(msg) => write!(f, "Process error: {}", msg),
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
