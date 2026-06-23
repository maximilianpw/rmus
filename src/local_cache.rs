use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::{config::LocalSource, sources::song::Song};

#[derive(Debug, Clone)]
pub struct LocalTrackCache {
    path: PathBuf,
    tracks: HashMap<PathBuf, CachedLocalTrack>,
    album_discoveries: Vec<CachedAlbumDiscovery>,
    dirty: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalAlbumCache {
    path: PathBuf,
    tracks: Vec<CachedLocalTrack>,
    discoveries: Vec<CachedAlbumDiscovery>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedLocalAlbumEntry {
    pub name: String,
    pub path: PathBuf,
    pub scope: CachedLocalAlbumScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachedLocalAlbumScope {
    Direct,
    Recursive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalDirectorySnapshot {
    path: PathBuf,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LocalCacheFile {
    #[serde(default)]
    tracks: Vec<CachedLocalTrack>,
    #[serde(default)]
    album_discoveries: Vec<CachedAlbumDiscovery>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedAlbumDiscovery {
    #[serde(default)]
    sources: Vec<CachedAlbumSource>,
    #[serde(default)]
    directories: Vec<CachedDirectorySnapshot>,
    #[serde(default)]
    entries: Vec<CachedAlbumEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct CachedAlbumSource {
    name: String,
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedDirectorySnapshot {
    path: String,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedAlbumEntry {
    name: String,
    path: String,
    scope: CachedAlbumScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CachedAlbumScope {
    Direct,
    Recursive,
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
        let cache = read_cache_file(&path);
        let album_discoveries = cache.album_discoveries;
        let tracks = cache
            .tracks
            .into_iter()
            .filter(|track| !track.path.trim().is_empty())
            .map(|track| (PathBuf::from(&track.path), track))
            .collect();

        Self {
            path,
            tracks,
            album_discoveries,
            dirty: false,
        }
    }

    fn save(&self) -> Result<(), std::io::Error> {
        let mut tracks: Vec<_> = self.tracks.values().cloned().collect();
        tracks.sort_by(|a, b| a.path.cmp(&b.path));
        write_cache_file(
            &self.path,
            LocalCacheFile {
                tracks,
                album_discoveries: self.album_discoveries.clone(),
            },
        )
    }
}

impl Default for LocalAlbumCache {
    fn default() -> Self {
        Self::load_from_path(default_local_cache_path())
    }
}

impl LocalAlbumCache {
    #[cfg(test)]
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self::load_from_path(path)
    }

    pub(crate) fn album_entries_for_sources(
        &self,
        sources: &[LocalSource],
    ) -> Option<Vec<CachedLocalAlbumEntry>> {
        let source_key = source_cache_key(sources);
        let discovery = self
            .discoveries
            .iter()
            .find(|discovery| discovery.sources == source_key)?;

        if !sources.is_empty() && discovery.directories.is_empty() {
            return None;
        }

        if !discovery
            .directories
            .iter()
            .all(CachedDirectorySnapshot::matches_current)
        {
            return None;
        }

        Some(
            discovery
                .entries
                .iter()
                .map(CachedLocalAlbumEntry::from_cached)
                .collect(),
        )
    }

    pub(crate) fn save_album_entries(
        &mut self,
        sources: &[LocalSource],
        entries: &[CachedLocalAlbumEntry],
        directories: &[LocalDirectorySnapshot],
    ) -> Result<(), std::io::Error> {
        let source_key = source_cache_key(sources);
        let discovery = CachedAlbumDiscovery {
            sources: source_key.clone(),
            directories: directories
                .iter()
                .map(CachedDirectorySnapshot::from_snapshot)
                .collect(),
            entries: entries.iter().map(CachedAlbumEntry::from_entry).collect(),
        };

        self.discoveries
            .retain(|existing| existing.sources != source_key);
        self.discoveries.push(discovery);
        self.discoveries.sort_by(|a, b| a.sources.cmp(&b.sources));
        self.save()
    }

    fn load_from_path(path: PathBuf) -> Self {
        let cache = read_cache_file(&path);
        Self {
            path,
            tracks: cache.tracks,
            discoveries: cache.album_discoveries,
        }
    }

    fn save(&self) -> Result<(), std::io::Error> {
        write_cache_file(
            &self.path,
            LocalCacheFile {
                tracks: self.tracks.clone(),
                album_discoveries: self.discoveries.clone(),
            },
        )
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

impl LocalDirectorySnapshot {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        if !metadata.is_dir() {
            return None;
        }
        let modified = metadata.modified().ok()?;
        let modified = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        Some(Self {
            path: path.to_path_buf(),
            modified_secs: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
        })
    }
}

impl CachedDirectorySnapshot {
    fn from_snapshot(snapshot: &LocalDirectorySnapshot) -> Self {
        Self {
            path: snapshot.path.to_string_lossy().into_owned(),
            modified_secs: snapshot.modified_secs,
            modified_nanos: snapshot.modified_nanos,
        }
    }

    fn matches_current(&self) -> bool {
        let Some(current) = LocalDirectorySnapshot::from_path(Path::new(&self.path)) else {
            return false;
        };

        current.modified_secs == self.modified_secs && current.modified_nanos == self.modified_nanos
    }
}

impl CachedAlbumEntry {
    fn from_entry(entry: &CachedLocalAlbumEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.to_string_lossy().into_owned(),
            scope: match entry.scope {
                CachedLocalAlbumScope::Direct => CachedAlbumScope::Direct,
                CachedLocalAlbumScope::Recursive => CachedAlbumScope::Recursive,
            },
        }
    }
}

impl CachedLocalAlbumEntry {
    fn from_cached(entry: &CachedAlbumEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: PathBuf::from(&entry.path),
            scope: match entry.scope {
                CachedAlbumScope::Direct => CachedLocalAlbumScope::Direct,
                CachedAlbumScope::Recursive => CachedLocalAlbumScope::Recursive,
            },
        }
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

fn source_cache_key(sources: &[LocalSource]) -> Vec<CachedAlbumSource> {
    sources
        .iter()
        .map(|source| CachedAlbumSource {
            name: source.name.clone(),
            path: source.path.to_string_lossy().into_owned(),
        })
        .collect()
}

fn read_cache_file(path: &Path) -> LocalCacheFile {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str::<LocalCacheFile>(&content).ok())
        .unwrap_or_default()
}

fn write_cache_file(path: &Path, cache: LocalCacheFile) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content =
        toml::to_string_pretty(&cache).map_err(|error| std::io::Error::other(error.to_string()))?;
    fs::write(path, content)
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

    #[test]
    fn album_cache_write_preserves_cached_track_metadata() {
        let root = test_path("album-root");
        fs::create_dir_all(&root).unwrap();
        let file = root.join("01 - Track.flac");
        fs::write(&file, "audio").unwrap();
        let cache_path = test_path("shared-cache");

        let identity = FileIdentity::from_path(&file).unwrap();
        let cached_song = Song {
            path: file.clone(),
            title: "Cached Track".to_string(),
            ..Default::default()
        };
        let mut track_cache = LocalTrackCache::with_path(cache_path.clone());
        track_cache.tracks.insert(
            file.clone(),
            CachedLocalTrack::from_song(&cached_song, identity),
        );
        track_cache.dirty = true;
        track_cache.save_if_dirty().unwrap();

        let mut album_cache = LocalAlbumCache::with_path(cache_path.clone());
        album_cache
            .save_album_entries(
                &[LocalSource {
                    name: "Library".to_string(),
                    path: root.clone(),
                }],
                &[CachedLocalAlbumEntry {
                    name: "Library".to_string(),
                    path: root.clone(),
                    scope: CachedLocalAlbumScope::Direct,
                }],
                &[LocalDirectorySnapshot::from_path(&root).unwrap()],
            )
            .unwrap();

        let mut loaded = LocalTrackCache::with_path(cache_path.clone());
        let song = loaded.song_for_path(&file);

        assert_eq!(song.title, "Cached Track");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn album_cache_rejects_nonempty_sources_without_directory_snapshots() {
        let source = LocalSource {
            name: "Missing".to_string(),
            path: PathBuf::from("/definitely/missing/rmus/source"),
        };
        let cache = LocalAlbumCache {
            path: test_path("snapshotless-cache"),
            tracks: Vec::new(),
            discoveries: vec![CachedAlbumDiscovery {
                sources: source_cache_key(std::slice::from_ref(&source)),
                directories: Vec::new(),
                entries: vec![CachedAlbumEntry {
                    name: "Missing".to_string(),
                    path: source.path.to_string_lossy().into_owned(),
                    scope: CachedAlbumScope::Recursive,
                }],
            }],
        };

        assert!(
            cache
                .album_entries_for_sources(std::slice::from_ref(&source))
                .is_none(),
            "snapshotless discoveries for configured sources cannot prove the source is unchanged"
        );
    }
}
