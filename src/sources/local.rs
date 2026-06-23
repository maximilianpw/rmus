#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::{
    cmp::Ordering,
    fmt::Debug,
    fs::{self},
    path::{Path, PathBuf},
};

use crate::{
    config::LocalSource,
    local_cache::{
        CachedLocalAlbumEntry, CachedLocalAlbumScope, LocalAlbumCache, LocalDirectorySnapshot,
        LocalTrackCache,
    },
    sources::{song::Song, MusicSource},
};

#[derive(Debug, Default)]
pub struct LocalFiles {
    pub name: String,
    pub files: Vec<LocalSource>,
    album_entries: Vec<LocalAlbumEntry>,
}

const SUPPORTED_AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "wav", "m4a", "aac", "wma", "alac", "aiff", "ape", "mka", "wv",
];

#[cfg(test)]
static SONG_SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static ALBUM_DISCOVERY_SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn reset_song_scan_count() {
    SONG_SCAN_COUNT.store(0, AtomicOrdering::SeqCst);
}

#[cfg(test)]
pub(crate) fn song_scan_count() -> usize {
    SONG_SCAN_COUNT.load(AtomicOrdering::SeqCst)
}

#[cfg(test)]
pub(crate) fn reset_album_discovery_scan_count() {
    ALBUM_DISCOVERY_SCAN_COUNT.store(0, AtomicOrdering::SeqCst);
}

#[cfg(test)]
pub(crate) fn album_discovery_scan_count() -> usize {
    ALBUM_DISCOVERY_SCAN_COUNT.load(AtomicOrdering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalAlbumEntry {
    name: String,
    path: PathBuf,
    scope: LocalAlbumScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalAlbumScope {
    Direct,
    Recursive,
}

#[derive(Debug, Clone)]
struct LocalAlbumDiscoveryResult {
    entries: Vec<LocalAlbumEntry>,
    directories: Vec<LocalDirectorySnapshot>,
}

fn is_supported_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            SUPPORTED_AUDIO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
        .unwrap_or(false)
}

fn collect_audio_files(path: &Path, songs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            collect_audio_files(&path, songs);
        } else if file_type.is_file() && is_supported_audio_file(&path) {
            songs.push(path);
        }
    }
}

fn collect_direct_audio_files(path: &Path, songs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_file() && is_supported_audio_file(&path) {
            songs.push(path);
        }
    }
}

fn read_album_discovery_dir(
    path: &Path,
    snapshots: &mut Vec<LocalDirectorySnapshot>,
) -> Option<fs::ReadDir> {
    if let Some(snapshot) = LocalDirectorySnapshot::from_path(path) {
        snapshots.push(snapshot);
    }
    #[cfg(test)]
    ALBUM_DISCOVERY_SCAN_COUNT.fetch_add(1, AtomicOrdering::SeqCst);
    fs::read_dir(path).ok()
}

fn directory_contains_direct_audio(
    path: &Path,
    snapshots: &mut Vec<LocalDirectorySnapshot>,
) -> bool {
    let Some(entries) = read_album_discovery_dir(path, snapshots) else {
        return false;
    };

    entries.filter_map(|entry| entry.ok()).any(|entry| {
        entry.file_type().is_ok_and(|file_type| file_type.is_file())
            && is_supported_audio_file(&entry.path())
    })
}

fn local_album_entry_name(source_root: &Path, album_path: &Path) -> String {
    let relative = album_path.strip_prefix(source_root).unwrap_or(album_path);
    let label = relative
        .iter()
        .filter_map(|part| part.to_str())
        .collect::<Vec<_>>()
        .join(" / ");

    if !label.is_empty() {
        return label;
    }

    album_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| album_path.to_string_lossy().into_owned())
}

fn collect_child_album_entries(
    source_root: &Path,
    path: &Path,
    entries: &mut Vec<LocalAlbumEntry>,
    snapshots: &mut Vec<LocalDirectorySnapshot>,
) {
    let Some(read_dir) = read_album_discovery_dir(path, snapshots) else {
        return;
    };

    let mut child_dirs: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|file_type| file_type.is_dir())
                .map(|_| entry.path())
        })
        .collect();
    child_dirs.sort_by(|a, b| {
        a.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            .cmp(
                &b.file_name()
                    .map(|name| name.to_string_lossy().to_lowercase())
                    .unwrap_or_default(),
            )
            .then_with(|| a.cmp(b))
    });

    for child_dir in child_dirs {
        if directory_contains_direct_audio(&child_dir, snapshots) {
            entries.push(LocalAlbumEntry {
                name: local_album_entry_name(source_root, &child_dir),
                path: child_dir.clone(),
                scope: LocalAlbumScope::Direct,
            });
        }
        collect_child_album_entries(source_root, &child_dir, entries, snapshots);
    }
}

fn discover_album_entries_uncached(sources: &[LocalSource]) -> LocalAlbumDiscoveryResult {
    let mut discovered = Vec::new();
    let mut directories = Vec::new();

    for source in sources {
        let mut source_entries = Vec::new();
        if directory_contains_direct_audio(&source.path, &mut directories) {
            source_entries.push(LocalAlbumEntry {
                name: source.name.clone(),
                path: source.path.clone(),
                scope: LocalAlbumScope::Direct,
            });
        }

        collect_child_album_entries(
            &source.path,
            &source.path,
            &mut source_entries,
            &mut directories,
        );

        if source_entries.is_empty() {
            source_entries.push(LocalAlbumEntry {
                name: source.name.clone(),
                path: source.path.clone(),
                scope: LocalAlbumScope::Recursive,
            });
        }

        discovered.extend(source_entries);
    }

    LocalAlbumDiscoveryResult {
        entries: discovered,
        directories,
    }
}

fn discover_album_entries(sources: &[LocalSource]) -> Vec<LocalAlbumEntry> {
    let mut cache = LocalAlbumCache::default();
    discover_album_entries_with_cache(sources, &mut cache)
}

fn discover_album_entries_with_cache(
    sources: &[LocalSource],
    cache: &mut LocalAlbumCache,
) -> Vec<LocalAlbumEntry> {
    if let Some(entries) = cache.album_entries_for_sources(sources) {
        return entries
            .into_iter()
            .map(LocalAlbumEntry::from_cached)
            .collect();
    }

    let result = discover_album_entries_uncached(sources);
    let cached_entries: Vec<_> = result
        .entries
        .iter()
        .map(LocalAlbumEntry::to_cached)
        .collect();
    let _ = cache.save_album_entries(sources, &cached_entries, &result.directories);
    result.entries
}

fn discover_album_entries_fresh(sources: &[LocalSource]) -> Vec<LocalAlbumEntry> {
    let result = discover_album_entries_uncached(sources);
    let mut cache = LocalAlbumCache::default();
    let cached_entries: Vec<_> = result
        .entries
        .iter()
        .map(LocalAlbumEntry::to_cached)
        .collect();
    let _ = cache.save_album_entries(sources, &cached_entries, &result.directories);
    result.entries
}

impl LocalAlbumEntry {
    fn from_cached(entry: CachedLocalAlbumEntry) -> Self {
        Self {
            name: entry.name,
            path: entry.path,
            scope: match entry.scope {
                CachedLocalAlbumScope::Direct => LocalAlbumScope::Direct,
                CachedLocalAlbumScope::Recursive => LocalAlbumScope::Recursive,
            },
        }
    }

    fn to_cached(&self) -> CachedLocalAlbumEntry {
        CachedLocalAlbumEntry {
            name: self.name.clone(),
            path: self.path.clone(),
            scope: match self.scope {
                LocalAlbumScope::Direct => CachedLocalAlbumScope::Direct,
                LocalAlbumScope::Recursive => CachedLocalAlbumScope::Recursive,
            },
        }
    }
}

impl MusicSource for LocalFiles {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn get_albums(&self) -> Vec<String> {
        self.album_entries
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    }

    fn get_album_path(&self, index: usize) -> Option<PathBuf> {
        self.album_entries
            .get(index)
            .map(|entry| entry.path.clone())
    }

    fn get_songs_from_album(&self, path: PathBuf) -> Vec<Song> {
        self.album_entries
            .iter()
            .find(|entry| paths_equivalent(&entry.path, &path))
            .map(Self::songs_for_entry)
            .unwrap_or_else(|| Self::songs_from_path(path))
    }
}

impl LocalFiles {
    pub fn new(name: String, files: Vec<LocalSource>) -> Box<Self> {
        let album_entries = discover_album_entries(&files);
        Box::new(LocalFiles {
            name,
            files,
            album_entries,
        })
    }

    pub fn new_fresh(name: String, files: Vec<LocalSource>) -> Box<Self> {
        let album_entries = discover_album_entries_fresh(&files);
        Box::new(LocalFiles {
            name,
            files,
            album_entries,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_with_album_cache(
        name: String,
        files: Vec<LocalSource>,
        cache: &mut LocalAlbumCache,
    ) -> Box<Self> {
        let album_entries = discover_album_entries_with_cache(&files, cache);
        Box::new(LocalFiles {
            name,
            files,
            album_entries,
        })
    }

    pub fn album_for_path(
        sources: &[LocalSource],
        path: &Path,
    ) -> Option<(PathBuf, String, Vec<Song>)> {
        discover_album_entries(sources)
            .into_iter()
            .find(|entry| paths_equivalent(&entry.path, path))
            .map(|entry| {
                let songs = Self::songs_for_entry(&entry);
                (entry.path, entry.name, songs)
            })
    }

    pub fn songs_from_path(path: PathBuf) -> Vec<Song> {
        let mut files = Vec::new();
        collect_audio_files(&path, &mut files);
        Self::songs_from_files(files)
    }

    fn songs_directly_from_path(path: PathBuf) -> Vec<Song> {
        let mut files = Vec::new();
        collect_direct_audio_files(&path, &mut files);
        Self::songs_from_files(files)
    }

    fn songs_for_entry(entry: &LocalAlbumEntry) -> Vec<Song> {
        match entry.scope {
            LocalAlbumScope::Direct => Self::songs_directly_from_path(entry.path.clone()),
            LocalAlbumScope::Recursive => Self::songs_from_path(entry.path.clone()),
        }
    }

    fn songs_from_files(files: Vec<PathBuf>) -> Vec<Song> {
        let mut cache = LocalTrackCache::default();
        Self::songs_from_files_with_cache(files, &mut cache)
    }

    pub(crate) fn songs_from_files_with_cache(
        mut files: Vec<PathBuf>,
        cache: &mut LocalTrackCache,
    ) -> Vec<Song> {
        #[cfg(test)]
        SONG_SCAN_COUNT.fetch_add(files.len(), AtomicOrdering::SeqCst);

        files.sort_by(|a, b| {
            a.file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default()
                .cmp(
                    &b.file_name()
                        .map(|name| name.to_string_lossy().to_lowercase())
                        .unwrap_or_default(),
                )
        });
        let mut songs: Vec<Song> = files
            .into_iter()
            .map(|path| cache.song_for_path(&path))
            .collect();
        let _ = cache.save_if_dirty();
        Self::sort_songs_for_playback(&mut songs);
        songs
    }

    pub(crate) fn sort_songs_for_playback(songs: &mut [Song]) {
        songs.sort_by(compare_songs_for_playback);
    }
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    matches!(
        (left.canonicalize(), right.canonicalize()),
        (Ok(left), Ok(right)) if left == right
    )
}

fn compare_songs_for_playback(a: &Song, b: &Song) -> Ordering {
    song_album_sort_key(a)
        .cmp(&song_album_sort_key(b))
        .then_with(|| compare_optional_numbers(a.disc_number, b.disc_number))
        .then_with(|| compare_optional_numbers(a.track_number, b.track_number))
        .then_with(|| song_title_sort_key(a).cmp(&song_title_sort_key(b)))
        .then_with(|| a.path.cmp(&b.path))
}

fn compare_optional_numbers(a: Option<u32>, b: Option<u32>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn song_album_sort_key(song: &Song) -> String {
    let album = song.album_name.trim();
    if !album.is_empty() {
        return album.to_lowercase();
    }

    song.path
        .parent()
        .map(|path| path.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn song_title_sort_key(song: &Song) -> String {
    let title = song.title.trim();
    if !title.is_empty() {
        return title.to_lowercase();
    }

    song.path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| song.path.to_string_lossy().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LocalSource;
    use crate::local_cache::{LocalAlbumCache, LocalTrackCache};
    use std::{
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rmus-local-source-{name}-{nanos}"))
    }

    fn test_cache_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rmus-local-source-cache-{name}-{nanos}.toml"))
    }

    fn escaped_toml_string(value: &str) -> String {
        value.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn write_cache_entry(cache_path: &Path, song_path: &Path, title: &str) {
        let metadata = fs::metadata(song_path).unwrap();
        let modified = metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        fs::write(
            cache_path,
            format!(
                r#"
                    [[tracks]]
                    path = "{}"
                    len = {}
                    modified_secs = {}
                    modified_nanos = {}
                    title = "{}"
                "#,
                escaped_toml_string(&song_path.to_string_lossy()),
                metadata.len(),
                modified.as_secs(),
                modified.subsec_nanos(),
                escaped_toml_string(title)
            ),
        )
        .unwrap();
    }

    #[test]
    fn local_source_lists_supported_audio_files_sorted() {
        let dir = test_dir("audio-only");
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("02 - Second.flac"), "").unwrap();
        fs::write(dir.join("cover.jpg"), "").unwrap();
        fs::write(dir.join("nested").join("03 - Nested.opus"), "").unwrap();
        fs::write(dir.join("01 - First.MP3"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();

        let source = LocalFiles::new("Local".to_string(), Vec::new());
        let songs = source.get_songs_from_album(dir.clone());
        let titles: Vec<String> = songs.into_iter().map(|song| song.title).collect();

        assert_eq!(
            titles,
            vec!["01 - First.MP3", "02 - Second.flac", "03 - Nested.opus"]
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn local_source_discovers_child_album_directories() {
        let dir = test_dir("album-directories");
        fs::create_dir_all(dir.join("Artist A").join("Debut")).unwrap();
        fs::create_dir_all(dir.join("Zeta")).unwrap();
        fs::create_dir_all(dir.join("Docs")).unwrap();
        fs::write(
            dir.join("Artist A").join("Debut").join("01 - Opener.flac"),
            "",
        )
        .unwrap();
        fs::write(dir.join("Zeta").join("01 - Last.flac"), "").unwrap();
        fs::write(dir.join("Docs").join("notes.txt"), "").unwrap();

        let source = LocalFiles::new(
            "Local".to_string(),
            vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        );

        assert_eq!(
            source.get_albums(),
            vec!["Artist A / Debut".to_string(), "Zeta".to_string()]
        );
        assert_eq!(
            source.get_album_path(0),
            Some(dir.join("Artist A").join("Debut"))
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn local_source_keeps_source_entry_for_direct_tracks() {
        let dir = test_dir("direct-tracks");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("01 - Single.flac"), "").unwrap();

        let source = LocalFiles::new(
            "Local".to_string(),
            vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        );

        assert_eq!(source.get_albums(), vec!["Library".to_string()]);
        assert_eq!(source.get_album_path(0), Some(dir.clone()));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn local_source_entry_excludes_child_album_tracks() {
        let dir = test_dir("direct-and-child-tracks");
        fs::create_dir_all(dir.join("Child Album")).unwrap();
        fs::write(dir.join("00 - Root Track.flac"), "").unwrap();
        fs::write(dir.join("Child Album").join("01 - Child Track.flac"), "").unwrap();

        let source = LocalFiles::new(
            "Local".to_string(),
            vec![LocalSource {
                name: "Library".to_string(),
                path: dir.clone(),
            }],
        );

        assert_eq!(
            source.get_albums(),
            vec!["Library".to_string(), "Child Album".to_string()]
        );

        let root_songs = source.get_songs_from_album(source.get_album_path(0).unwrap());
        let child_songs = source.get_songs_from_album(source.get_album_path(1).unwrap());

        assert_eq!(
            root_songs
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            vec!["00 - Root Track.flac"]
        );
        assert_eq!(
            child_songs
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            vec!["01 - Child Track.flac"]
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn local_source_uses_cached_album_discovery_for_unchanged_sources() {
        let dir = test_dir("cached-album-discovery");
        fs::create_dir_all(dir.join("Alpha")).unwrap();
        fs::write(dir.join("Alpha").join("01 - Alpha.flac"), "").unwrap();
        let sources = vec![LocalSource {
            name: "Library".to_string(),
            path: dir.clone(),
        }];
        let cache_path = test_cache_path("album-discovery");

        let mut cache = LocalAlbumCache::with_path(cache_path.clone());
        reset_album_discovery_scan_count();
        let first =
            LocalFiles::new_with_album_cache("Local".to_string(), sources.clone(), &mut cache);
        assert_eq!(first.get_albums(), vec!["Alpha".to_string()]);
        assert!(
            album_discovery_scan_count() > 0,
            "first discovery should walk local directories"
        );

        let mut cache = LocalAlbumCache::with_path(cache_path.clone());
        reset_album_discovery_scan_count();
        let second = LocalFiles::new_with_album_cache("Local".to_string(), sources, &mut cache);

        assert_eq!(second.get_albums(), vec!["Alpha".to_string()]);
        assert_eq!(
            album_discovery_scan_count(),
            0,
            "unchanged sources should reuse cached album entries without walking directories"
        );

        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn local_source_invalidates_cached_album_discovery_when_directory_changes() {
        let dir = test_dir("cached-album-discovery-invalidated");
        fs::create_dir_all(dir.join("Alpha")).unwrap();
        fs::write(dir.join("Alpha").join("01 - Alpha.flac"), "").unwrap();
        let sources = vec![LocalSource {
            name: "Library".to_string(),
            path: dir.clone(),
        }];
        let cache_path = test_cache_path("album-discovery-invalidated");

        let mut cache = LocalAlbumCache::with_path(cache_path.clone());
        let first =
            LocalFiles::new_with_album_cache("Local".to_string(), sources.clone(), &mut cache);
        assert_eq!(first.get_albums(), vec!["Alpha".to_string()]);

        thread::sleep(Duration::from_millis(10));
        fs::create_dir_all(dir.join("Beta")).unwrap();
        fs::write(dir.join("Beta").join("01 - Beta.flac"), "").unwrap();

        let mut cache = LocalAlbumCache::with_path(cache_path.clone());
        reset_album_discovery_scan_count();
        let refreshed = LocalFiles::new_with_album_cache("Local".to_string(), sources, &mut cache);

        assert!(
            album_discovery_scan_count() > 0,
            "directory changes should invalidate cached album discovery"
        );
        assert_eq!(
            refreshed.get_albums(),
            vec!["Alpha".to_string(), "Beta".to_string()]
        );

        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn local_source_uses_cached_track_metadata_for_unchanged_file() {
        let dir = test_dir("cached-metadata");
        fs::create_dir_all(&dir).unwrap();
        let song_path = dir.join("01 - Untagged.flac");
        fs::write(&song_path, "audio").unwrap();
        let cache_path = test_cache_path("cached-metadata");
        write_cache_entry(&cache_path, &song_path, "Cached Title");
        let mut cache = LocalTrackCache::with_path(cache_path.clone());

        let songs = LocalFiles::songs_from_files_with_cache(vec![song_path.clone()], &mut cache);

        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "Cached Title");

        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn local_source_reloads_metadata_when_cached_file_changes() {
        let dir = test_dir("stale-cache");
        fs::create_dir_all(&dir).unwrap();
        let song_path = dir.join("01 - Untagged.flac");
        fs::write(&song_path, "audio").unwrap();
        let cache_path = test_cache_path("stale-cache");
        write_cache_entry(&cache_path, &song_path, "Stale Cached Title");
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(&song_path, "changed audio").unwrap();
        let mut cache = LocalTrackCache::with_path(cache_path.clone());

        let songs = LocalFiles::songs_from_files_with_cache(vec![song_path.clone()], &mut cache);

        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "01 - Untagged.flac");
        assert_ne!(songs[0].title, "Stale Cached Title");

        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_file(cache_path);
    }

    #[test]
    fn local_song_sort_prefers_track_numbers_when_available() {
        let mut songs = vec![
            Song {
                title: "Alphabetical First".to_string(),
                path: PathBuf::from("/music/album/alpha.flac"),
                track_number: Some(2),
                ..Default::default()
            },
            Song {
                title: "Track One".to_string(),
                path: PathBuf::from("/music/album/zulu.flac"),
                track_number: Some(1),
                ..Default::default()
            },
            Song {
                title: "Untagged".to_string(),
                path: PathBuf::from("/music/album/untagged.flac"),
                ..Default::default()
            },
        ];

        LocalFiles::sort_songs_for_playback(&mut songs);

        let titles: Vec<String> = songs.into_iter().map(|song| song.title).collect();
        assert_eq!(titles, vec!["Track One", "Alphabetical First", "Untagged"]);
    }

    #[test]
    fn local_song_sort_groups_disc_numbers_before_track_numbers() {
        let mut songs = vec![
            Song {
                title: "Disc 2 Track 1".to_string(),
                album_name: "Album".to_string(),
                path: PathBuf::from("/music/album/disc2-track1.flac"),
                disc_number: Some(2),
                track_number: Some(1),
                ..Default::default()
            },
            Song {
                title: "Disc 1 Track 2".to_string(),
                album_name: "Album".to_string(),
                path: PathBuf::from("/music/album/disc1-track2.flac"),
                disc_number: Some(1),
                track_number: Some(2),
                ..Default::default()
            },
            Song {
                title: "Disc 1 Track 1".to_string(),
                album_name: "Album".to_string(),
                path: PathBuf::from("/music/album/disc1-track1.flac"),
                disc_number: Some(1),
                track_number: Some(1),
                ..Default::default()
            },
        ];

        LocalFiles::sort_songs_for_playback(&mut songs);

        let titles: Vec<String> = songs.into_iter().map(|song| song.title).collect();
        assert_eq!(
            titles,
            vec!["Disc 1 Track 1", "Disc 1 Track 2", "Disc 2 Track 1"]
        );
    }
}
