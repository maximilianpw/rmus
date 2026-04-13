use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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

fn playlists_dir() -> PathBuf {
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

impl Playlist {
    pub fn new(name: String) -> Self {
        Self {
            name,
            tracks: Vec::new(),
        }
    }

    pub fn load_all() -> Vec<Playlist> {
        let dir = playlists_dir();
        if !dir.exists() {
            return Vec::new();
        }

        let mut playlists = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
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

    pub fn save(&self) -> Result<(), std::io::Error> {
        let dir = playlists_dir();
        fs::create_dir_all(&dir)?;

        let filename = format!("{}.toml", sanitize_filename(&self.name));
        let path = dir.join(filename);
        let toml_string =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        fs::write(&path, toml_string)
    }

    pub fn delete(name: &str) -> Result<(), std::io::Error> {
        let dir = playlists_dir();
        let filename = format!("{}.toml", sanitize_filename(name));
        let path = dir.join(filename);
        if path.exists() {
            fs::remove_file(path)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
