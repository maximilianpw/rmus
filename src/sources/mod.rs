use std::{fmt::Debug, path::PathBuf};

use crate::playlist::Playlist;
use crate::sources::song::Song;

pub mod local;
pub mod qobuz;
pub mod song;
pub mod streaming;
pub mod tidal;

pub trait MusicSource {
    fn name(&self) -> String;
    fn get_albums(&self) -> Vec<String>;
    fn get_album_path(&self, index: usize) -> Option<PathBuf>;
    fn get_songs_from_album(&self, path: PathBuf) -> Vec<Song>;
}

impl Debug for dyn MusicSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Music Source: name = {}", self.name())
    }
}

/// Placeholder source for streaming service tabs in the left panel.
/// Returns no albums — streaming services use search instead.
pub struct StreamingTab {
    service_name: String,
}

impl StreamingTab {
    pub fn boxed(name: &str) -> Box<dyn MusicSource> {
        Box::new(Self {
            service_name: name.to_string(),
        })
    }
}

impl MusicSource for StreamingTab {
    fn name(&self) -> String {
        self.service_name.clone()
    }

    fn get_albums(&self) -> Vec<String> {
        vec![]
    }

    fn get_album_path(&self, _index: usize) -> Option<PathBuf> {
        None
    }

    fn get_songs_from_album(&self, _path: PathBuf) -> Vec<Song> {
        vec![]
    }
}

/// Source backed by saved playlists from ~/.config/rmus/playlists/.
pub struct PlaylistSource {
    playlists: Vec<Playlist>,
}

impl PlaylistSource {
    pub fn new() -> Self {
        Self {
            playlists: Playlist::load_all(),
        }
    }

    pub fn reload(&mut self) {
        self.playlists = Playlist::load_all();
    }

    pub fn get_playlist(&self, index: usize) -> Option<&Playlist> {
        self.playlists.get(index)
    }
}

impl MusicSource for PlaylistSource {
    fn name(&self) -> String {
        "Playlists".to_string()
    }

    fn get_albums(&self) -> Vec<String> {
        self.playlists
            .iter()
            .map(|p| format!("{} ({} tracks)", p.name, p.tracks.len()))
            .collect()
    }

    fn get_album_path(&self, index: usize) -> Option<PathBuf> {
        // Use the index as a sentinel — we don't need real paths for playlists
        self.playlists
            .get(index)
            .map(|_| PathBuf::from(format!("playlist:{}", index)))
    }

    fn get_songs_from_album(&self, path: PathBuf) -> Vec<Song> {
        let path_str = path.to_string_lossy();
        let index: usize = path_str
            .strip_prefix("playlist:")
            .and_then(|s| s.parse().ok())
            .unwrap_or(usize::MAX);
        let Some(playlist) = self.playlists.get(index) else {
            return Vec::new();
        };

        playlist
            .tracks
            .iter()
            .map(|t| {
                if let Some(ref file_path) = t.path {
                    Song {
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        album_name: t.album_name.clone(),
                        path: PathBuf::from(file_path),
                        ..Default::default()
                    }
                } else {
                    // Stream track — title only, needs URL resolution at play time
                    Song {
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        album_name: t.album_name.clone(),
                        ..Default::default()
                    }
                }
            })
            .collect()
    }
}
