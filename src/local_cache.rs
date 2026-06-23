use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::sources::song::Song;

#[derive(Debug, Clone)]
pub struct LocalTrackCache {
    path: PathBuf,
    tracks: HashMap<PathBuf, CachedLocalTrack>,
    dirty: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalTrackCacheFile {
    #[serde(default)]
    tracks: Vec<CachedLocalTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedLocalTrack {
    path: String,
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

impl Default for LocalTrackCache {
    fn default() -> Self {
        Self::load_from_path(default_local_cache_path())
    }
}

impl LocalTrackCache {
    pub fn default_path() -> PathBuf {
        default_local_cache_path()
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self::load_from_path(path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn song_for_path(&mut self, path: &Path) -> Song {
        let Some(identity) = FileIdentity::from_path(path) else {
            return Song::from_path(path.to_path_buf());
        };

        if let Some(cached) = self
            .tracks
            .get(path)
            .filter(|cached| cached.identity() == identity)
        {
            return cached.to_song();
        }

        let song = Song::from_path(path.to_path_buf());
        self.tracks.insert(
            path.to_path_buf(),
            CachedLocalTrack::from_song(&song, identity),
        );
        self.dirty = true;
        song
    }

    pub fn save_if_dirty(&mut self) -> Result<(), std::io::Error> {
        if !self.dirty {
            return Ok(());
        }
        self.save()?;
        self.dirty = false;
        Ok(())
    }

    fn load_from_path(path: PathBuf) -> Self {
        let tracks = fs::read_to_string(&path)
            .ok()
            .and_then(|content| toml::from_str::<LocalTrackCacheFile>(&content).ok())
            .map(|cache| {
                cache
                    .tracks
                    .into_iter()
                    .filter(|track| !track.path.trim().is_empty())
                    .map(|track| (PathBuf::from(&track.path), track))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            path,
            tracks,
            dirty: false,
        }
    }

    fn save(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut tracks: Vec<_> = self.tracks.values().cloned().collect();
        tracks.sort_by(|a, b| a.path.cmp(&b.path));
        let cache = LocalTrackCacheFile { tracks };
        let content = toml::to_string_pretty(&cache)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        fs::write(&self.path, content)
    }
}

impl FileIdentity {
    fn from_path(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?;
        let modified = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        Some(Self {
            len: metadata.len(),
            modified_secs: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
        })
    }
}

impl CachedLocalTrack {
    fn identity(&self) -> FileIdentity {
        FileIdentity {
            len: self.len,
            modified_secs: self.modified_secs,
            modified_nanos: self.modified_nanos,
        }
    }

    fn from_song(song: &Song, identity: FileIdentity) -> Self {
        Self {
            path: song.path.to_string_lossy().into_owned(),
            len: identity.len,
            modified_secs: identity.modified_secs,
            modified_nanos: identity.modified_nanos,
            title: song.title.clone(),
            artist: song.artist.clone(),
            album_name: song.album_name.clone(),
            disc_number: song.disc_number,
            track_number: song.track_number,
            duration_secs: song.duration_secs,
        }
    }

    fn to_song(&self) -> Song {
        Song {
            path: PathBuf::from(&self.path),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album_name: self.album_name.clone(),
            disc_number: self.disc_number,
            track_number: self.track_number,
            duration_secs: self.duration_secs,
            ..Default::default()
        }
    }
}

fn default_local_cache_path() -> PathBuf {
    ProjectDirs::from("com", "maximilianpw", "rmus")
        .map(|dirs| dirs.config_dir().join("local-cache.toml"))
        .unwrap_or_else(|| PathBuf::from("local-cache.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    fn test_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rmus-local-cache-{name}-{nanos}.toml"))
    }

    #[test]
    fn cache_reuses_metadata_for_unchanged_file() {
        let file = test_path("unchanged-song").with_extension("flac");
        fs::write(&file, "audio").unwrap();
        let cache_path = test_path("unchanged-cache");
        let mut cache = LocalTrackCache::with_path(cache_path.clone());
        let identity = FileIdentity::from_path(&file).unwrap();
        let cached_song = Song {
            path: file.clone(),
            title: "Cached Title".to_string(),
            artist: "Cached Artist".to_string(),
            album_name: "Cached Album".to_string(),
            disc_number: Some(1),
            track_number: Some(2),
            duration_secs: Some(123.0),
            ..Default::default()
        };
        cache.tracks.insert(
            file.clone(),
            CachedLocalTrack::from_song(&cached_song, identity),
        );
        cache.dirty = true;
        cache.save_if_dirty().unwrap();

        let mut loaded = LocalTrackCache::with_path(cache_path.clone());
        let song = loaded.song_for_path(&file);

        assert_eq!(song.title, "Cached Title");
        assert_eq!(song.artist, "Cached Artist");
        assert_eq!(song.album_name, "Cached Album");
        assert_eq!(song.disc_number, Some(1));
        assert_eq!(song.track_number, Some(2));
        assert_eq!(song.duration_secs, Some(123.0));

        let _ = fs::remove_file(file);
        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn cache_reparses_changed_file() {
        let file = test_path("changed-song").with_extension("flac");
        fs::write(&file, "audio").unwrap();
        let cache_path = test_path("changed-cache");
        let mut cache = LocalTrackCache::with_path(cache_path.clone());
        let identity = FileIdentity::from_path(&file).unwrap();
        let cached_song = Song {
            path: file.clone(),
            title: "Stale Cached Title".to_string(),
            ..Default::default()
        };
        cache.tracks.insert(
            file.clone(),
            CachedLocalTrack::from_song(&cached_song, identity),
        );
        cache.dirty = true;
        cache.save_if_dirty().unwrap();

        thread::sleep(Duration::from_millis(2));
        fs::write(&file, "changed audio").unwrap();
        let mut loaded = LocalTrackCache::with_path(cache_path.clone());
        let song = loaded.song_for_path(&file);

        assert_eq!(song.title, file.file_name().unwrap().to_string_lossy());
        assert_ne!(song.title, "Stale Cached Title");

        let _ = fs::remove_file(file);
        let _ = fs::remove_file(cache_path);
    }
}
