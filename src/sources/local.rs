use std::{
    cmp::Ordering,
    fmt::Debug,
    fs::{self},
    path::{Path, PathBuf},
};

use crate::{
    config::LocalSource,
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

fn directory_contains_direct_audio(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else {
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
) {
    let Ok(read_dir) = fs::read_dir(path) else {
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
        if directory_contains_direct_audio(&child_dir) {
            entries.push(LocalAlbumEntry {
                name: local_album_entry_name(source_root, &child_dir),
                path: child_dir.clone(),
                scope: LocalAlbumScope::Direct,
            });
        }
        collect_child_album_entries(source_root, &child_dir, entries);
    }
}

fn discover_album_entries(sources: &[LocalSource]) -> Vec<LocalAlbumEntry> {
    let mut discovered = Vec::new();

    for source in sources {
        let mut source_entries = Vec::new();
        if directory_contains_direct_audio(&source.path) {
            source_entries.push(LocalAlbumEntry {
                name: source.name.clone(),
                path: source.path.clone(),
                scope: LocalAlbumScope::Direct,
            });
        }

        collect_child_album_entries(&source.path, &source.path, &mut source_entries);

        if source_entries.is_empty() {
            source_entries.push(LocalAlbumEntry {
                name: source.name.clone(),
                path: source.path.clone(),
                scope: LocalAlbumScope::Recursive,
            });
        }

        discovered.extend(source_entries);
    }

    discovered
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

    fn songs_from_files(mut files: Vec<PathBuf>) -> Vec<Song> {
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
        let mut songs: Vec<Song> = files.into_iter().map(Song::from_path).collect();
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rmus-local-source-{name}-{nanos}"))
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
