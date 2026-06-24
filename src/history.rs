use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    sources::song::{Song, StreamManifest},
    utils::rmus_config_dir,
};

#[derive(Debug, Clone)]
pub struct HistoryStore {
    path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryFile {
    #[serde(default)]
    tracks: Vec<HistoryTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryTrack {
    title: String,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disc_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    track_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_quality: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_manifest_contents: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_manifest_file_extension: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_service: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_track_id: Option<String>,
}

impl Default for HistoryStore {
    fn default() -> Self {
        Self {
            path: default_history_path(),
        }
    }
}

impl HistoryStore {
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Vec<Song> {
        let Ok(content) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };

        toml::from_str::<HistoryFile>(&content)
            .map(|history| history.tracks.iter().map(song_from_track).collect())
            .unwrap_or_default()
    }

    pub fn save(&self, songs: &[Song]) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let history = HistoryFile {
            tracks: songs.iter().map(track_from_song).collect(),
        };
        let content = toml::to_string_pretty(&history)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        fs::write(&self.path, content)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn default_history_path() -> PathBuf {
    rmus_config_dir().join("history.toml")
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn track_from_song(song: &Song) -> HistoryTrack {
    let manifest = song.stream_manifest.as_ref();
    HistoryTrack {
        title: song.title.clone(),
        artist: song.artist.clone(),
        album_name: song.album_name.clone(),
        disc_number: song.disc_number,
        track_number: song.track_number,
        duration_secs: song.duration_secs,
        stream_quality: song.stream_quality.clone(),
        path: if song.path.as_os_str().is_empty() {
            None
        } else {
            Some(song.path.to_string_lossy().into_owned())
        },
        url: song.url.as_deref().and_then(non_empty_string),
        stream_manifest_contents: manifest.map(|manifest| manifest.contents.clone()),
        stream_manifest_file_extension: manifest.map(|manifest| manifest.file_extension.clone()),
        stream_service: song.stream_service.as_deref().and_then(non_empty_string),
        stream_track_id: song.stream_track_id.as_deref().and_then(non_empty_string),
    }
}

fn song_from_track(track: &HistoryTrack) -> Song {
    let stream_manifest = match (
        track.stream_manifest_contents.clone(),
        track.stream_manifest_file_extension.clone(),
    ) {
        (Some(contents), Some(file_extension)) if !file_extension.trim().is_empty() => {
            Some(StreamManifest {
                contents,
                file_extension,
            })
        }
        _ => None,
    };

    Song {
        title: track.title.clone(),
        artist: track.artist.clone(),
        album_name: track.album_name.clone(),
        disc_number: track.disc_number,
        track_number: track.track_number,
        duration_secs: track.duration_secs,
        stream_quality: track.stream_quality.clone(),
        path: track
            .path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_default(),
        url: track.url.clone(),
        stream_manifest,
        stream_service: track.stream_service.clone(),
        stream_track_id: track.stream_track_id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rmus-history-{name}-{nanos}.toml"))
    }

    #[test]
    fn store_loads_empty_history_when_file_is_missing() {
        let store = HistoryStore::with_path(test_path("missing"));

        assert!(store.load().is_empty());
    }

    #[test]
    fn store_round_trips_local_and_stream_history() {
        let path = test_path("round-trip");
        let store = HistoryStore::with_path(path.clone());
        let songs = vec![
            Song {
                title: "Local Song".to_string(),
                artist: "Local Artist".to_string(),
                album_name: "Local Album".to_string(),
                disc_number: Some(1),
                track_number: Some(2),
                duration_secs: Some(123.0),
                path: PathBuf::from("/music/local.flac"),
                ..Default::default()
            },
            Song {
                title: "Stream Song".to_string(),
                artist: "Stream Artist".to_string(),
                album_name: "Stream Album".to_string(),
                url: Some("https://stream.example.com/song.flac".to_string()),
                stream_manifest: Some(StreamManifest {
                    contents: "#EXTM3U".to_string(),
                    file_extension: "m3u8".to_string(),
                }),
                stream_quality: Some("Hi-Res".to_string()),
                stream_service: Some("Qobuz".to_string()),
                stream_track_id: Some("track-1".to_string()),
                ..Default::default()
            },
        ];

        store.save(&songs).unwrap();
        let loaded = store.load();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "Local Song");
        assert_eq!(loaded[0].path, PathBuf::from("/music/local.flac"));
        assert_eq!(loaded[0].duration_secs, Some(123.0));
        assert_eq!(loaded[1].title, "Stream Song");
        assert_eq!(
            loaded[1].url.as_deref(),
            Some("https://stream.example.com/song.flac")
        );
        assert_eq!(
            loaded[1]
                .stream_manifest
                .as_ref()
                .map(|manifest| manifest.file_extension.as_str()),
            Some("m3u8")
        );
        assert_eq!(loaded[1].stream_service.as_deref(), Some("Qobuz"));
        assert_eq!(loaded[1].stream_track_id.as_deref(), Some("track-1"));

        let _ = fs::remove_file(path);
    }
}
