use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::sources::song::{Song, StreamManifest};

#[derive(Debug, Clone)]
pub struct QueueStore {
    path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct QueueState {
    pub tracks: Vec<Song>,
    pub position: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct QueueFile {
    #[serde(default)]
    position: usize,
    #[serde(default)]
    tracks: Vec<QueueTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueueTrack {
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

impl QueueState {
    pub fn new(tracks: Vec<Song>, position: usize) -> Self {
        let position = clamped_position(tracks.len(), position);
        Self { tracks, position }
    }
}

impl Default for QueueStore {
    fn default() -> Self {
        Self {
            path: default_queue_path(),
        }
    }
}

impl QueueStore {
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> QueueState {
        let Ok(content) = fs::read_to_string(&self.path) else {
            return QueueState::default();
        };

        toml::from_str::<QueueFile>(&content)
            .map(|queue| {
                let tracks: Vec<Song> = queue.tracks.iter().map(song_from_track).collect();
                QueueState::new(tracks, queue.position)
            })
            .unwrap_or_default()
    }

    pub fn save(&self, state: &QueueState) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let queue = QueueFile {
            position: clamped_position(state.tracks.len(), state.position),
            tracks: state.tracks.iter().map(track_from_song).collect(),
        };
        let content = toml::to_string_pretty(&queue)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        fs::write(&self.path, content)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn default_queue_path() -> PathBuf {
    ProjectDirs::from("com", "maximilianpw", "rmus")
        .map(|dirs| dirs.config_dir().join("queue.toml"))
        .unwrap_or_else(|| PathBuf::from("queue.toml"))
}

fn clamped_position(track_count: usize, position: usize) -> usize {
    if track_count == 0 {
        0
    } else {
        position.min(track_count - 1)
    }
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn track_from_song(song: &Song) -> QueueTrack {
    let manifest = song.stream_manifest.as_ref();
    QueueTrack {
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

fn song_from_track(track: &QueueTrack) -> Song {
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
        std::env::temp_dir().join(format!("rmus-queue-{name}-{nanos}.toml"))
    }

    #[test]
    fn store_loads_empty_queue_when_file_is_missing() {
        let store = QueueStore::with_path(test_path("missing"));

        let state = store.load();

        assert!(state.tracks.is_empty());
        assert_eq!(state.position, 0);
    }

    #[test]
    fn store_round_trips_local_and_stream_queue_state() {
        let path = test_path("round-trip");
        let store = QueueStore::with_path(path.clone());
        let state = QueueState::new(
            vec![
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
            ],
            1,
        );

        store.save(&state).unwrap();
        let loaded = store.load();

        assert_eq!(loaded.position, 1);
        assert_eq!(loaded.tracks.len(), 2);
        assert_eq!(loaded.tracks[0].title, "Local Song");
        assert_eq!(loaded.tracks[0].path, PathBuf::from("/music/local.flac"));
        assert_eq!(loaded.tracks[0].duration_secs, Some(123.0));
        assert_eq!(loaded.tracks[1].title, "Stream Song");
        assert_eq!(
            loaded.tracks[1].url.as_deref(),
            Some("https://stream.example.com/song.flac")
        );
        assert_eq!(
            loaded.tracks[1]
                .stream_manifest
                .as_ref()
                .map(|manifest| manifest.file_extension.as_str()),
            Some("m3u8")
        );
        assert_eq!(loaded.tracks[1].stream_service.as_deref(), Some("Qobuz"));
        assert_eq!(loaded.tracks[1].stream_track_id.as_deref(), Some("track-1"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn store_clamps_loaded_position_to_last_track() {
        let path = test_path("clamp");
        let store = QueueStore::with_path(path.clone());
        fs::write(
            &path,
            r#"
                position = 99

                [[tracks]]
                title = "First"

                [[tracks]]
                title = "Second"
            "#,
        )
        .unwrap();

        let loaded = store.load();

        assert_eq!(loaded.position, 1);
        assert_eq!(loaded.tracks.len(), 2);

        let _ = fs::remove_file(path);
    }
}
