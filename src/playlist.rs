use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::sources::{song::Song, MusicSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<PlaylistTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTrack {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album_name: String,
    /// Local file path (for local tracks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Streaming service name (e.g. "Qobuz", "Tidal").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_service: Option<String>,
    /// Track ID on the streaming service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_track_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistSummary {
    pub name: String,
    pub track_count: usize,
}

impl PlaylistSummary {
    pub fn display_title(&self) -> String {
        format!("{} ({} tracks)", self.name, self.track_count)
    }
}

#[derive(Debug, Clone)]
pub struct PlaylistStore {
    dir: PathBuf,
}

/// Source backed by saved playlists from the configured playlist directory.
#[derive(Debug)]
pub struct PlaylistSource {
    store: PlaylistStore,
    playlists: Vec<Playlist>,
}

fn default_playlists_dir() -> PathBuf {
    ProjectDirs::from("com", "maximilianpw", "rmus")
        .map(|dirs| dirs.config_dir().join("playlists"))
        .unwrap_or_else(|| PathBuf::from("playlists"))
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn playlist_path_for_index(index: usize) -> PathBuf {
    PathBuf::from(format!("playlist:{}", index))
}

fn playlist_index_from_path(path: PathBuf) -> Option<usize> {
    path.to_string_lossy()
        .strip_prefix("playlist:")
        .and_then(|s| s.parse().ok())
}

fn track_from_song(song: &Song) -> PlaylistTrack {
    PlaylistTrack {
        title: song.title.clone(),
        artist: song.artist.clone(),
        album_name: song.album_name.clone(),
        path: if song.path.as_os_str().is_empty() {
            None
        } else {
            Some(song.path.to_string_lossy().into_owned())
        },
        stream_service: None,
        stream_track_id: None,
    }
}

fn song_from_track(track: &PlaylistTrack) -> Song {
    if let Some(ref file_path) = track.path {
        Song {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album_name: track.album_name.clone(),
            path: PathBuf::from(file_path),
            ..Default::default()
        }
    } else {
        Song {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album_name: track.album_name.clone(),
            ..Default::default()
        }
    }
}

impl Playlist {
    pub fn new(name: String) -> Self {
        Self {
            name,
            tracks: Vec::new(),
        }
    }

    pub fn load_all() -> Vec<Playlist> {
        PlaylistStore::default().load_all()
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        self.save_to_dir(&default_playlists_dir())
    }

    pub fn delete(name: &str) -> Result<(), std::io::Error> {
        Self::delete_from_dir(&default_playlists_dir(), name)
    }

    fn save_to_dir(&self, dir: &Path) -> Result<(), std::io::Error> {
        fs::create_dir_all(dir)?;

        let filename = format!("{}.toml", sanitize_filename(&self.name));
        let path = dir.join(filename);
        let toml_string =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        fs::write(&path, toml_string)
    }

    fn delete_from_dir(dir: &Path, name: &str) -> Result<(), std::io::Error> {
        let filename = format!("{}.toml", sanitize_filename(name));
        let path = dir.join(filename);
        if path.exists() {
            fs::remove_file(path)
        } else {
            Ok(())
        }
    }
}

impl Default for PlaylistStore {
    fn default() -> Self {
        Self {
            dir: default_playlists_dir(),
        }
    }
}

impl PlaylistStore {
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn load_all(&self) -> Vec<Playlist> {
        if !self.dir.exists() {
            return Vec::new();
        }

        let mut playlists = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "toml") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(playlist) = toml::from_str::<Playlist>(&content) {
                            playlists.push(playlist);
                        }
                    }
                }
            }
        }
        playlists.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        playlists
    }

    pub fn summaries(&self) -> Vec<PlaylistSummary> {
        self.load_all().iter().map(Self::summary_for).collect()
    }

    pub fn display_rows(&self) -> Vec<String> {
        self.summaries()
            .iter()
            .map(PlaylistSummary::display_title)
            .collect()
    }

    pub fn playlist_names(&self) -> Vec<String> {
        self.load_all().iter().map(|p| p.name.clone()).collect()
    }

    pub fn create(&self, name: String) -> Result<(), std::io::Error> {
        Playlist::new(name).save_to_dir(&self.dir)
    }

    pub fn delete_at(&self, index: usize) -> Result<Option<String>, std::io::Error> {
        let playlists = self.load_all();
        let Some(playlist) = playlists.get(index) else {
            return Ok(None);
        };
        let name = playlist.name.clone();
        Playlist::delete_from_dir(&self.dir, &name)?;
        Ok(Some(name))
    }

    pub fn songs_for_index(&self, index: usize) -> Vec<Song> {
        self.load_all()
            .get(index)
            .map(Self::songs_for_playlist)
            .unwrap_or_default()
    }

    pub fn add_songs_to_index(
        &self,
        index: usize,
        songs: &[Song],
    ) -> Result<Option<(String, usize)>, std::io::Error> {
        let mut playlists = self.load_all();
        let Some(playlist) = playlists.get_mut(index) else {
            return Ok(None);
        };
        for song in songs {
            playlist.tracks.push(track_from_song(song));
        }
        let name = playlist.name.clone();
        let count = songs.len();
        playlist.save_to_dir(&self.dir)?;
        Ok(Some((name, count)))
    }

    fn summary_for(playlist: &Playlist) -> PlaylistSummary {
        PlaylistSummary {
            name: playlist.name.clone(),
            track_count: playlist.tracks.len(),
        }
    }

    fn songs_for_playlist(playlist: &Playlist) -> Vec<Song> {
        playlist.tracks.iter().map(song_from_track).collect()
    }
}

impl PlaylistSource {
    pub fn new() -> Self {
        Self::with_store(PlaylistStore::default())
    }

    pub fn with_store(store: PlaylistStore) -> Self {
        let playlists = store.load_all();
        Self { store, playlists }
    }

    pub fn reload(&mut self) {
        self.playlists = self.store.load_all();
    }

    pub fn get_playlist(&self, index: usize) -> Option<&Playlist> {
        self.playlists.get(index)
    }
}

impl Default for PlaylistSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicSource for PlaylistSource {
    fn name(&self) -> String {
        "Playlists".to_string()
    }

    fn get_albums(&self) -> Vec<String> {
        self.playlists
            .iter()
            .map(PlaylistStore::summary_for)
            .map(|summary| summary.display_title())
            .collect()
    }

    fn get_album_path(&self, index: usize) -> Option<PathBuf> {
        self.playlists
            .get(index)
            .map(|_| playlist_path_for_index(index))
    }

    fn get_songs_from_album(&self, path: PathBuf) -> Vec<Song> {
        playlist_index_from_path(path)
            .and_then(|index| self.playlists.get(index))
            .map(PlaylistStore::songs_for_playlist)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rmus-playlist-test-{name}-{nanos}"))
    }

    #[test]
    fn playlist_round_trip() {
        let playlist = Playlist {
            name: "Test Playlist".to_string(),
            tracks: vec![
                PlaylistTrack {
                    title: "Local Song".to_string(),
                    artist: "Artist".to_string(),
                    album_name: "Album".to_string(),
                    path: Some("/music/song.flac".to_string()),
                    stream_service: None,
                    stream_track_id: None,
                },
                PlaylistTrack {
                    title: "Stream Song".to_string(),
                    artist: "Artist 2".to_string(),
                    album_name: "Album 2".to_string(),
                    path: None,
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("12345".to_string()),
                },
            ],
        };

        let toml_string = toml::to_string_pretty(&playlist).unwrap();
        let loaded: Playlist = toml::from_str(&toml_string).unwrap();

        assert_eq!(loaded.name, "Test Playlist");
        assert_eq!(loaded.tracks.len(), 2);
        assert_eq!(loaded.tracks[0].title, "Local Song");
        assert_eq!(loaded.tracks[0].path.as_deref(), Some("/music/song.flac"));
        assert!(loaded.tracks[0].stream_service.is_none());
        assert_eq!(loaded.tracks[1].title, "Stream Song");
        assert_eq!(loaded.tracks[1].stream_service.as_deref(), Some("Qobuz"));
        assert_eq!(loaded.tracks[1].stream_track_id.as_deref(), Some("12345"));
    }

    #[test]
    fn sanitize_filename_removes_special_chars() {
        assert_eq!(sanitize_filename("My Playlist!"), "My Playlist_");
        assert_eq!(sanitize_filename("test/path"), "test_path");
        assert_eq!(sanitize_filename("normal-name_2"), "normal-name_2");
    }

    #[test]
    fn store_adds_songs_to_playlist_by_index() {
        let dir = test_dir("add");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();

        let songs = vec![Song {
            title: "Track".to_string(),
            artist: "Artist".to_string(),
            album_name: "Album".to_string(),
            path: "/music/track.flac".into(),
            ..Default::default()
        }];
        let result = store.add_songs_to_index(0, &songs).unwrap();

        assert_eq!(result, Some(("Mix".to_string(), 1)));
        let loaded = store.load_all();
        assert_eq!(loaded[0].tracks.len(), 1);
        assert_eq!(
            loaded[0].tracks[0].path.as_deref(),
            Some("/music/track.flac")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn playlist_source_hides_playlist_path_encoding() {
        let dir = test_dir("source");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[Song {
                    title: "Track".to_string(),
                    artist: "Artist".to_string(),
                    path: "/music/track.flac".into(),
                    ..Default::default()
                }],
            )
            .unwrap();
        let source = PlaylistSource::with_store(store);

        assert_eq!(source.get_albums(), vec!["Mix (1 tracks)"]);
        let path = source.get_album_path(0).unwrap();
        let songs = source.get_songs_from_album(path);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "Track");

        let _ = fs::remove_dir_all(dir);
    }
}
