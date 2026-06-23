use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::{
    sources::{song::Song, MusicSource},
    utils::track_count_label,
};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_quality: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistAddSummary {
    pub playlist_name: String,
    pub added_count: usize,
    pub skipped_count: usize,
    pub total_count: usize,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRemoveSummary {
    pub playlist_name: String,
    pub removed_count: usize,
    pub missed_count: usize,
    pub total_count: usize,
    pub existed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistImportSummary {
    pub playlist_name: String,
    pub imported_count: usize,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistExportSummary {
    pub playlist_name: String,
    pub path: PathBuf,
    pub exported_count: usize,
    pub skipped_count: usize,
}

impl PlaylistSummary {
    pub fn display_title(&self) -> String {
        format!("{} ({})", self.name, track_count_label(self.track_count))
    }
}

impl PlaylistImportSummary {
    pub fn message(&self) -> String {
        match self.skipped_count {
            0 => format!(
                "Imported {} into playlist '{}'",
                track_count_label(self.imported_count),
                self.playlist_name
            ),
            skipped => format!(
                "Imported {} into playlist '{}' (skipped {})",
                track_count_label(self.imported_count),
                self.playlist_name,
                track_count_label(skipped)
            ),
        }
    }
}

impl PlaylistExportSummary {
    pub fn message(&self) -> String {
        let target = self.path.to_string_lossy();
        match self.skipped_count {
            0 => format!(
                "Exported {} from playlist '{}' to {}",
                track_count_label(self.exported_count),
                self.playlist_name,
                target
            ),
            skipped => format!(
                "Exported {} from playlist '{}' to {} (skipped {})",
                track_count_label(self.exported_count),
                self.playlist_name,
                target,
                track_count_label(skipped)
            ),
        }
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

fn playlist_path_for_name(name: &str) -> PathBuf {
    PathBuf::from(format!("playlist:{}", sanitize_filename(name)))
}

fn playlist_key_from_path(path: PathBuf) -> Option<String> {
    path.to_string_lossy()
        .strip_prefix("playlist:")
        .map(ToString::to_string)
}

fn track_from_song(song: &Song) -> PlaylistTrack {
    PlaylistTrack {
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
        stream_service: song.stream_service.clone(),
        stream_track_id: song.stream_track_id.clone(),
    }
}

fn track_matches_song(track: &PlaylistTrack, song: &Song) -> bool {
    if let (Some(track_service), Some(track_id), Some(song_service), Some(song_id)) = (
        track.stream_service.as_deref(),
        track.stream_track_id.as_deref(),
        song.stream_service.as_deref(),
        song.stream_track_id.as_deref(),
    ) {
        if !track_service.trim().is_empty()
            && !track_id.trim().is_empty()
            && track_service.eq_ignore_ascii_case(song_service)
            && track_id == song_id
        {
            return true;
        }
    }

    if let Some(track_path) = track.path.as_deref().filter(|path| !path.trim().is_empty()) {
        if !song.path.as_os_str().is_empty() && Path::new(track_path) == song.path {
            return true;
        }
    }

    let title = song.title.trim();
    !title.is_empty()
        && track.title.trim() == title
        && track.artist.trim() == song.artist.trim()
        && track.album_name.trim() == song.album_name.trim()
}

fn song_from_track(track: &PlaylistTrack) -> Song {
    if let Some(ref file_path) = track.path {
        Song {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album_name: track.album_name.clone(),
            disc_number: track.disc_number,
            track_number: track.track_number,
            duration_secs: track.duration_secs,
            stream_quality: track.stream_quality.clone(),
            path: PathBuf::from(file_path),
            stream_service: track.stream_service.clone(),
            stream_track_id: track.stream_track_id.clone(),
            ..Default::default()
        }
    } else {
        Song {
            title: track.title.clone(),
            artist: track.artist.clone(),
            album_name: track.album_name.clone(),
            disc_number: track.disc_number,
            track_number: track.track_number,
            duration_secs: track.duration_secs,
            stream_quality: track.stream_quality.clone(),
            stream_service: track.stream_service.clone(),
            stream_track_id: track.stream_track_id.clone(),
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
        self.write_to_dir(dir, false)
    }

    fn create_in_dir(&self, dir: &Path) -> Result<(), std::io::Error> {
        self.write_to_dir(dir, true)
    }

    fn write_to_dir(&self, dir: &Path, fail_if_exists: bool) -> Result<(), std::io::Error> {
        fs::create_dir_all(dir)?;

        let filename = format!("{}.toml", sanitize_filename(&self.name));
        let path = dir.join(filename);
        if fail_if_exists && path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Playlist '{}' already exists", self.name),
            ));
        }

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

    pub fn dir(&self) -> &Path {
        &self.dir
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

    pub fn is_playlist_path(path: &Path) -> bool {
        playlist_key_from_path(path.to_path_buf()).is_some()
    }

    pub fn path_for_name(name: &str) -> PathBuf {
        playlist_path_for_name(name)
    }

    pub fn create(&self, name: String) -> Result<(), std::io::Error> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            ));
        }

        if self
            .load_all()
            .iter()
            .any(|playlist| playlist.name.eq_ignore_ascii_case(&name))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Playlist '{}' already exists", name),
            ));
        }

        Playlist::new(name).create_in_dir(&self.dir)
    }

    pub fn import_m3u(
        &self,
        path: &Path,
        name_override: Option<&str>,
    ) -> Result<PlaylistImportSummary, std::io::Error> {
        let name = playlist_name_for_import(path, name_override)?;
        if self
            .load_all()
            .iter()
            .any(|playlist| playlist.name.eq_ignore_ascii_case(&name))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Playlist '{}' already exists", name),
            ));
        }

        let imported = parse_m3u(path)?;
        if imported.songs.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "M3U playlist did not contain any local tracks",
            ));
        }

        let playlist = Playlist {
            name: name.clone(),
            tracks: imported.songs.iter().map(track_from_song).collect(),
        };
        playlist.create_in_dir(&self.dir)?;

        Ok(PlaylistImportSummary {
            playlist_name: name,
            imported_count: imported.songs.len(),
            skipped_count: imported.skipped_count,
        })
    }

    pub fn export_m3u(
        &self,
        name: &str,
        path: &Path,
    ) -> Result<PlaylistExportSummary, std::io::Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            ));
        }

        let playlists = self.load_all();
        let Some(playlist) = playlists
            .iter()
            .find(|playlist| playlist.name.eq_ignore_ascii_case(name))
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Playlist '{}' not found", name),
            ));
        };

        let exported = export_playlist_to_m3u(playlist, path)?;
        Ok(PlaylistExportSummary {
            playlist_name: playlist.name.clone(),
            path: path.to_path_buf(),
            exported_count: exported.exported_count,
            skipped_count: exported.skipped_count,
        })
    }

    pub fn rename_at(
        &self,
        index: usize,
        new_name: String,
    ) -> Result<Option<(String, String)>, std::io::Error> {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            ));
        }

        let playlists = self.load_all();
        let Some(playlist) = playlists.get(index).cloned() else {
            return Ok(None);
        };

        if playlists
            .iter()
            .enumerate()
            .any(|(playlist_index, playlist)| {
                playlist_index != index && playlist.name.eq_ignore_ascii_case(&new_name)
            })
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Playlist '{}' already exists", new_name),
            ));
        }

        let old_name = playlist.name.clone();
        if old_name == new_name {
            return Ok(Some((old_name, new_name)));
        }

        let old_filename = sanitize_filename(&old_name);
        let new_filename = sanitize_filename(&new_name);
        let old_path = self.dir.join(format!("{old_filename}.toml"));
        let new_path = self.dir.join(format!("{new_filename}.toml"));

        if !old_filename.eq_ignore_ascii_case(&new_filename) && new_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Playlist file '{}' already exists", new_filename),
            ));
        }

        let mut playlist = playlist;
        playlist.name = new_name.clone();
        if old_filename.eq_ignore_ascii_case(&new_filename) {
            playlist.save_to_dir(&self.dir)?;
        } else {
            playlist.create_in_dir(&self.dir)?;
            if old_path.exists() {
                fs::remove_file(old_path)?;
            }
        }

        Ok(Some((old_name, new_name)))
    }

    pub fn duplicate_at(
        &self,
        index: usize,
        new_name: String,
    ) -> Result<Option<(String, String)>, std::io::Error> {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            ));
        }

        let playlists = self.load_all();
        let Some(source) = playlists.get(index).cloned() else {
            return Ok(None);
        };

        if playlists
            .iter()
            .any(|playlist| playlist.name.eq_ignore_ascii_case(&new_name))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Playlist '{}' already exists", new_name),
            ));
        }

        let old_name = source.name.clone();
        let mut duplicate = source;
        duplicate.name = new_name.clone();
        duplicate.create_in_dir(&self.dir)?;
        Ok(Some((old_name, new_name)))
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

    pub fn songs_for_name(&self, name: &str) -> Vec<Song> {
        self.load_all()
            .iter()
            .find(|playlist| playlist.name == name)
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

    pub fn add_unique_songs_to_named_playlist(
        &self,
        name: &str,
        songs: &[Song],
    ) -> Result<PlaylistAddSummary, std::io::Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            ));
        }

        let mut playlists = self.load_all();
        let existing_index = playlists
            .iter()
            .position(|playlist| playlist.name.eq_ignore_ascii_case(name));
        let created = existing_index.is_none();
        let index = if let Some(index) = existing_index {
            index
        } else {
            playlists.push(Playlist::new(name.to_string()));
            playlists.len() - 1
        };

        let playlist = &mut playlists[index];
        let mut added_count = 0;
        let mut skipped_count = 0;
        for song in songs {
            if playlist
                .tracks
                .iter()
                .any(|track| track_matches_song(track, song))
            {
                skipped_count += 1;
            } else {
                playlist.tracks.push(track_from_song(song));
                added_count += 1;
            }
        }

        if added_count > 0 || created {
            if created {
                playlist.create_in_dir(&self.dir)?;
            } else {
                playlist.save_to_dir(&self.dir)?;
            }
        }

        Ok(PlaylistAddSummary {
            playlist_name: playlist.name.clone(),
            added_count,
            skipped_count,
            total_count: playlist.tracks.len(),
            created,
        })
    }

    pub fn remove_matching_songs_from_named_playlist(
        &self,
        name: &str,
        songs: &[Song],
    ) -> Result<PlaylistRemoveSummary, std::io::Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            ));
        }

        let mut playlists = self.load_all();
        let Some(index) = playlists
            .iter()
            .position(|playlist| playlist.name.eq_ignore_ascii_case(name))
        else {
            return Ok(PlaylistRemoveSummary {
                playlist_name: name.to_string(),
                removed_count: 0,
                missed_count: songs.len(),
                total_count: 0,
                existed: false,
            });
        };

        let playlist = &mut playlists[index];
        let mut removed_count = 0;
        let mut missed_count = 0;
        for song in songs {
            let before = playlist.tracks.len();
            playlist
                .tracks
                .retain(|track| !track_matches_song(track, song));
            let removed = before.saturating_sub(playlist.tracks.len());
            if removed > 0 {
                removed_count += removed;
            } else {
                missed_count += 1;
            }
        }

        if removed_count > 0 {
            playlist.save_to_dir(&self.dir)?;
        }

        Ok(PlaylistRemoveSummary {
            playlist_name: playlist.name.clone(),
            removed_count,
            missed_count,
            total_count: playlist.tracks.len(),
            existed: true,
        })
    }

    pub fn remove_song_from_path(
        &self,
        path: PathBuf,
        track_index: usize,
    ) -> Result<Option<(String, Song, Vec<Song>)>, std::io::Error> {
        let Some(playlist_key) = playlist_key_from_path(path) else {
            return Ok(None);
        };

        let mut playlists = self.load_all();
        let Some(playlist) = playlists
            .iter_mut()
            .find(|playlist| sanitize_filename(&playlist.name) == playlist_key)
        else {
            return Ok(None);
        };
        if track_index >= playlist.tracks.len() {
            return Ok(None);
        }

        let removed = playlist.tracks.remove(track_index);
        let name = playlist.name.clone();
        let removed_song = song_from_track(&removed);
        playlist.save_to_dir(&self.dir)?;
        let remaining_songs = Self::songs_for_playlist(playlist);
        Ok(Some((name, removed_song, remaining_songs)))
    }

    pub fn move_song_in_path(
        &self,
        path: PathBuf,
        from: usize,
        to: usize,
    ) -> Result<Option<(String, Song, Vec<Song>)>, std::io::Error> {
        let Some(playlist_key) = playlist_key_from_path(path) else {
            return Ok(None);
        };

        let mut playlists = self.load_all();
        let Some(playlist) = playlists
            .iter_mut()
            .find(|playlist| sanitize_filename(&playlist.name) == playlist_key)
        else {
            return Ok(None);
        };
        if from >= playlist.tracks.len() || to >= playlist.tracks.len() || from == to {
            return Ok(None);
        }

        let moved = playlist.tracks.remove(from);
        let moved_song = song_from_track(&moved);
        playlist.tracks.insert(to, moved);
        let name = playlist.name.clone();
        playlist.save_to_dir(&self.dir)?;
        let songs = Self::songs_for_playlist(playlist);
        Ok(Some((name, moved_song, songs)))
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

#[derive(Debug, Default)]
struct ImportedM3u {
    songs: Vec<Song>,
    skipped_count: usize,
}

#[derive(Debug, Default)]
struct ExportedM3u {
    exported_count: usize,
    skipped_count: usize,
}

#[derive(Debug, Default)]
struct ExtInf {
    title: Option<String>,
    duration_secs: Option<f64>,
}

fn playlist_name_for_import(
    path: &Path,
    name_override: Option<&str>,
) -> Result<String, std::io::Error> {
    let name = name_override
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|stem| !stem.is_empty())
                .map(ToString::to_string)
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Playlist name is required",
            )
        })?;

    Ok(name)
}

fn parse_m3u(path: &Path) -> Result<ImportedM3u, std::io::Error> {
    let bytes = fs::read(path)?;
    let content = String::from_utf8_lossy(&bytes);
    let base_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let mut imported = ImportedM3u::default();
    let mut pending_extinf = ExtInf::default();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(extinf) = parse_extinf(line) {
            pending_extinf = extinf;
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        if is_m3u_url(line) {
            imported.skipped_count += 1;
            pending_extinf = ExtInf::default();
            continue;
        }

        let track_path = resolve_m3u_track_path(base_dir, line);
        let title = pending_extinf
            .title
            .take()
            .unwrap_or_else(|| title_from_path(&track_path));
        let duration_secs = pending_extinf.duration_secs.take();
        imported.songs.push(Song {
            title,
            path: track_path,
            duration_secs,
            ..Default::default()
        });
    }

    Ok(imported)
}

fn parse_extinf(line: &str) -> Option<ExtInf> {
    let extinf_prefix = "#EXTINF:";
    if !line
        .get(..extinf_prefix.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(extinf_prefix))
    {
        return None;
    }

    let body = &line[extinf_prefix.len()..];
    let (duration_text, title_text) = body.split_once(',').unwrap_or((body, ""));
    let duration_secs = duration_text
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration > 0.0);
    let title = match title_text.trim() {
        "" => None,
        title => Some(title.to_string()),
    };

    Some(ExtInf {
        title,
        duration_secs,
    })
}

fn is_m3u_url(value: &str) -> bool {
    value
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
        || value
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
}

fn resolve_m3u_track_path(base_dir: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn title_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn export_playlist_to_m3u(playlist: &Playlist, path: &Path) -> Result<ExportedM3u, std::io::Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let mut output = String::from("#EXTM3U\n");
    let mut exported = ExportedM3u::default();

    for track in &playlist.tracks {
        let Some(track_path) = track
            .path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
        else {
            exported.skipped_count += 1;
            continue;
        };

        output.push_str(&format!(
            "#EXTINF:{},{}\n{}\n",
            m3u_duration_label(track.duration_secs),
            playlist_track_display_title(track),
            track_path
        ));
        exported.exported_count += 1;
    }

    if exported.exported_count == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Playlist does not contain any local tracks",
        ));
    }

    fs::write(path, output)?;
    Ok(exported)
}

fn m3u_duration_label(duration_secs: Option<f64>) -> String {
    duration_secs
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (duration.round() as i64).to_string())
        .unwrap_or_else(|| "-1".to_string())
}

fn playlist_track_display_title(track: &PlaylistTrack) -> String {
    let title = track.title.trim();
    let artist = track.artist.trim();
    match (artist.is_empty(), title.is_empty()) {
        (false, false) => format!("{artist} - {title}"),
        (true, false) => title.to_string(),
        (false, true) => artist.to_string(),
        (true, true) => track
            .path
            .as_deref()
            .map(Path::new)
            .map(title_from_path)
            .unwrap_or_else(|| "Unknown Track".to_string()),
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
            .map(|playlist| playlist_path_for_name(&playlist.name))
    }

    fn get_songs_from_album(&self, path: PathBuf) -> Vec<Song> {
        playlist_key_from_path(path)
            .and_then(|playlist_key| {
                self.playlists
                    .iter()
                    .find(|playlist| sanitize_filename(&playlist.name) == playlist_key)
            })
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
                    disc_number: None,
                    track_number: None,
                    duration_secs: None,
                    stream_quality: None,
                    path: Some("/music/song.flac".to_string()),
                    stream_service: None,
                    stream_track_id: None,
                },
                PlaylistTrack {
                    title: "Stream Song".to_string(),
                    artist: "Artist 2".to_string(),
                    album_name: "Album 2".to_string(),
                    disc_number: None,
                    track_number: None,
                    duration_secs: None,
                    stream_quality: None,
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
    fn store_adds_unique_songs_to_named_playlist_and_creates_it() {
        let dir = test_dir("add-unique-create");
        let store = PlaylistStore::with_dir(dir.clone());

        let result = store
            .add_unique_songs_to_named_playlist(
                "Favorites",
                &[Song {
                    title: "Track".to_string(),
                    artist: "Artist".to_string(),
                    album_name: "Album".to_string(),
                    path: "/music/track.flac".into(),
                    ..Default::default()
                }],
            )
            .unwrap();

        assert_eq!(
            result,
            PlaylistAddSummary {
                playlist_name: "Favorites".to_string(),
                added_count: 1,
                skipped_count: 0,
                total_count: 1,
                created: true,
            }
        );
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Favorites");
        assert_eq!(loaded[0].tracks.len(), 1);
        assert_eq!(
            loaded[0].tracks[0].path.as_deref(),
            Some("/music/track.flac")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_imports_local_m3u_playlist() {
        let dir = test_dir("import-m3u");
        fs::create_dir_all(dir.join("tracks")).unwrap();
        let playlist_file = dir.join("Road.m3u");
        fs::write(
            &playlist_file,
            "\
#EXTM3U
#EXTINF:123,Artist - First
tracks/first.flac
https://example.com/stream.flac
/absolute/second.mp3
",
        )
        .unwrap();
        let store = PlaylistStore::with_dir(dir.join("playlists"));

        let summary = store.import_m3u(&playlist_file, None).unwrap();

        assert_eq!(
            summary,
            PlaylistImportSummary {
                playlist_name: "Road".to_string(),
                imported_count: 2,
                skipped_count: 1,
            }
        );
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Road");
        assert_eq!(loaded[0].tracks.len(), 2);
        assert_eq!(loaded[0].tracks[0].title, "Artist - First");
        assert_eq!(loaded[0].tracks[0].duration_secs, Some(123.0));
        let relative_track = dir.join("tracks/first.flac").to_string_lossy().into_owned();
        assert_eq!(
            loaded[0].tracks[0].path.as_deref(),
            Some(relative_track.as_str())
        );
        assert_eq!(loaded[0].tracks[1].title, "second.mp3");
        assert_eq!(
            loaded[0].tracks[1].path.as_deref(),
            Some("/absolute/second.mp3")
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_imports_m3u_with_custom_name_without_overwriting() {
        let dir = test_dir("import-m3u-custom-name");
        let playlist_file = dir.join("source.m3u8");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&playlist_file, "track.flac\n").unwrap();
        let store = PlaylistStore::with_dir(dir.join("playlists"));

        let summary = store
            .import_m3u(&playlist_file, Some("Imported Mix"))
            .unwrap();

        assert_eq!(summary.playlist_name, "Imported Mix");
        assert_eq!(store.load_all()[0].name, "Imported Mix");
        let error = store
            .import_m3u(&playlist_file, Some("imported mix"))
            .expect_err("duplicate playlist names should be rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_exports_local_tracks_to_m3u() {
        let dir = test_dir("export-m3u");
        let store = PlaylistStore::with_dir(dir.join("playlists"));
        store.create("Road Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[
                    Song {
                        title: "Local Song".to_string(),
                        artist: "Artist".to_string(),
                        path: "/music/local.flac".into(),
                        duration_secs: Some(244.6),
                        ..Default::default()
                    },
                    Song {
                        title: "Stream Song".to_string(),
                        artist: "Streaming Artist".to_string(),
                        stream_service: Some("Qobuz".to_string()),
                        stream_track_id: Some("stream-1".to_string()),
                        ..Default::default()
                    },
                ],
            )
            .unwrap();

        let export_path = dir.join("exports/road.m3u8");
        let summary = store.export_m3u("road mix", &export_path).unwrap();

        assert_eq!(
            summary,
            PlaylistExportSummary {
                playlist_name: "Road Mix".to_string(),
                path: export_path.clone(),
                exported_count: 1,
                skipped_count: 1,
            }
        );
        let output = fs::read_to_string(&export_path).unwrap();
        assert_eq!(
            output,
            "\
#EXTM3U
#EXTINF:245,Artist - Local Song
/music/local.flac
"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_export_without_local_tracks() {
        let dir = test_dir("export-m3u-empty");
        let store = PlaylistStore::with_dir(dir.join("playlists"));
        store.create("Streams".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[Song {
                    title: "Stream Only".to_string(),
                    stream_service: Some("Tidal".to_string()),
                    stream_track_id: Some("stream-1".to_string()),
                    ..Default::default()
                }],
            )
            .unwrap();

        let error = store
            .export_m3u("Streams", &dir.join("streams.m3u8"))
            .expect_err("all-stream playlists should not write an empty M3U");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(!dir.join("streams.m3u8").exists());

        let error = store
            .export_m3u("Missing", &dir.join("missing.m3u8"))
            .expect_err("missing playlists should fail explicitly");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_skips_unique_song_duplicates_by_path_and_stream_reference() {
        let dir = test_dir("add-unique-skip");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Favorites".to_string()).unwrap();
        let local = Song {
            title: "Local Track".to_string(),
            artist: "Artist".to_string(),
            path: "/music/local.flac".into(),
            ..Default::default()
        };
        let stream = Song {
            title: "Stream Track".to_string(),
            artist: "Artist".to_string(),
            stream_service: Some("Qobuz".to_string()),
            stream_track_id: Some("stream-1".to_string()),
            ..Default::default()
        };

        store
            .add_unique_songs_to_named_playlist("Favorites", &[local.clone(), stream.clone()])
            .unwrap();
        let result = store
            .add_unique_songs_to_named_playlist("favorites", &[local, stream])
            .unwrap();

        assert_eq!(
            result,
            PlaylistAddSummary {
                playlist_name: "Favorites".to_string(),
                added_count: 0,
                skipped_count: 2,
                total_count: 2,
                created: false,
            }
        );
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].tracks.len(), 2);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_removes_matching_songs_from_named_playlist() {
        let dir = test_dir("remove-matching");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Favorites".to_string()).unwrap();
        let local = Song {
            title: "Local Track".to_string(),
            artist: "Artist".to_string(),
            path: "/music/local.flac".into(),
            ..Default::default()
        };
        let stream = Song {
            title: "Stream Track".to_string(),
            artist: "Artist".to_string(),
            stream_service: Some("Tidal".to_string()),
            stream_track_id: Some("stream-1".to_string()),
            ..Default::default()
        };
        let missing = Song {
            title: "Missing".to_string(),
            artist: "Artist".to_string(),
            path: "/music/missing.flac".into(),
            ..Default::default()
        };
        store
            .add_unique_songs_to_named_playlist("Favorites", &[local.clone(), stream.clone()])
            .unwrap();

        let result = store
            .remove_matching_songs_from_named_playlist("favorites", &[local, missing])
            .unwrap();

        assert_eq!(
            result,
            PlaylistRemoveSummary {
                playlist_name: "Favorites".to_string(),
                removed_count: 1,
                missed_count: 1,
                total_count: 1,
                existed: true,
            }
        );
        let loaded = store.load_all();
        assert_eq!(loaded[0].tracks.len(), 1);
        assert_eq!(
            loaded[0].tracks[0].stream_track_id.as_deref(),
            Some("stream-1")
        );

        let result = store
            .remove_matching_songs_from_named_playlist("Favorites", &[stream])
            .unwrap();
        assert_eq!(result.removed_count, 1);
        assert_eq!(result.total_count, 0);
        assert!(store.load_all()[0].tracks.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_reports_missing_named_playlist_when_removing_matches() {
        let dir = test_dir("remove-matching-missing-playlist");
        let store = PlaylistStore::with_dir(dir.clone());

        let result = store
            .remove_matching_songs_from_named_playlist(
                "Favorites",
                &[Song {
                    title: "Track".to_string(),
                    path: "/music/track.flac".into(),
                    ..Default::default()
                }],
            )
            .unwrap();

        assert_eq!(
            result,
            PlaylistRemoveSummary {
                playlist_name: "Favorites".to_string(),
                removed_count: 0,
                missed_count: 1,
                total_count: 0,
                existed: false,
            }
        );
        assert!(store.load_all().is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_preserves_playlist_track_playback_metadata() {
        let dir = test_dir("metadata");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();

        store
            .add_songs_to_index(
                0,
                &[Song {
                    title: "Track".to_string(),
                    artist: "Artist".to_string(),
                    album_name: "Album".to_string(),
                    path: "/music/track.flac".into(),
                    disc_number: Some(2),
                    track_number: Some(7),
                    duration_secs: Some(241.4),
                    stream_quality: Some("Hi-Res".to_string()),
                    ..Default::default()
                }],
            )
            .unwrap();

        let songs = store.songs_for_index(0);

        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].disc_number, Some(2));
        assert_eq!(songs[0].track_number, Some(7));
        assert_eq!(songs[0].duration_secs, Some(241.4));
        assert_eq!(songs[0].stream_quality.as_deref(), Some("Hi-Res"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_remove_returns_removed_song_metadata() {
        let dir = test_dir("remove-metadata");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[
                    Song {
                        title: "Remove Me".to_string(),
                        artist: "Artist".to_string(),
                        album_name: "Album".to_string(),
                        path: "/music/remove.flac".into(),
                        ..Default::default()
                    },
                    Song {
                        title: "Keep Me".to_string(),
                        artist: "Other Artist".to_string(),
                        path: "/music/keep.flac".into(),
                        ..Default::default()
                    },
                ],
            )
            .unwrap();

        let (_playlist_name, removed_song, remaining_songs) = store
            .remove_song_from_path(PathBuf::from("playlist:Mix"), 0)
            .unwrap()
            .unwrap();

        assert_eq!(removed_song.title, "Remove Me");
        assert_eq!(removed_song.artist, "Artist");
        assert_eq!(removed_song.album_name, "Album");
        assert_eq!(remaining_songs.len(), 1);
        assert_eq!(remaining_songs[0].title, "Keep Me");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_moves_song_within_playlist_by_path() {
        let dir = test_dir("move");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[
                    Song {
                        title: "First".to_string(),
                        artist: "Artist".to_string(),
                        path: "/music/first.flac".into(),
                        ..Default::default()
                    },
                    Song {
                        title: "Second".to_string(),
                        artist: "Artist".to_string(),
                        path: "/music/second.flac".into(),
                        ..Default::default()
                    },
                    Song {
                        title: "Third".to_string(),
                        artist: "Artist".to_string(),
                        path: "/music/third.flac".into(),
                        ..Default::default()
                    },
                ],
            )
            .unwrap();

        let (playlist_name, moved_song, songs) = store
            .move_song_in_path(PathBuf::from("playlist:Mix"), 1, 2)
            .unwrap()
            .unwrap();

        assert_eq!(playlist_name, "Mix");
        assert_eq!(moved_song.title, "Second");
        assert_eq!(
            songs
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Third", "Second"]
        );
        let loaded = store.load_all();
        assert_eq!(
            loaded[0]
                .tracks
                .iter()
                .map(|track| track.title.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Third", "Second"]
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_playlist_track_move_out_of_bounds_without_saving() {
        let dir = test_dir("move-oob");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[
                    Song {
                        title: "First".to_string(),
                        path: "/music/first.flac".into(),
                        ..Default::default()
                    },
                    Song {
                        title: "Second".to_string(),
                        path: "/music/second.flac".into(),
                        ..Default::default()
                    },
                ],
            )
            .unwrap();

        let result = store
            .move_song_in_path(PathBuf::from("playlist:Mix"), 0, 2)
            .unwrap();

        assert!(result.is_none());
        let loaded = store.load_all();
        assert_eq!(
            loaded[0]
                .tracks
                .iter()
                .map(|track| track.title.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second"]
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_duplicate_playlist_without_overwriting_tracks() {
        let dir = test_dir("duplicate");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[Song {
                    title: "Keep Me".to_string(),
                    artist: "Artist".to_string(),
                    path: "/music/keep.flac".into(),
                    ..Default::default()
                }],
            )
            .unwrap();

        let err = store.create("Mix".to_string()).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].tracks.len(), 1);
        assert_eq!(loaded[0].tracks[0].title, "Keep Me");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_case_insensitive_duplicate_playlist_name() {
        let dir = test_dir("duplicate-case");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Mix".to_string()).unwrap();

        let err = store.create("mix".to_string()).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Mix");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_blank_playlist_name() {
        let dir = test_dir("blank-name");
        let store = PlaylistStore::with_dir(dir.clone());

        let err = store.create("   ".to_string()).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(store.load_all().is_empty());
        assert!(
            !dir.join(".toml").exists(),
            "blank playlist names should not create hidden playlist files"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_trims_playlist_name_before_persisting() {
        let dir = test_dir("trimmed-name");
        let store = PlaylistStore::with_dir(dir.clone());

        store.create("  Mix  ".to_string()).unwrap();

        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Mix");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_renames_playlist_without_losing_tracks() {
        let dir = test_dir("rename");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Old Mix".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[Song {
                    title: "Keep Me".to_string(),
                    artist: "Artist".to_string(),
                    path: "/music/keep.flac".into(),
                    ..Default::default()
                }],
            )
            .unwrap();

        let result = store.rename_at(0, "  New Mix  ".to_string()).unwrap();

        assert_eq!(result, Some(("Old Mix".to_string(), "New Mix".to_string())));
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "New Mix");
        assert_eq!(loaded[0].tracks.len(), 1);
        assert_eq!(loaded[0].tracks[0].title, "Keep Me");
        assert!(
            !dir.join("Old Mix.toml").exists(),
            "rename should remove the old playlist file"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_duplicate_playlist_rename_without_overwriting() {
        let dir = test_dir("rename-duplicate");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Road".to_string()).unwrap();
        store.create("Sleep".to_string()).unwrap();

        let err = store.rename_at(0, "sleep".to_string()).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|playlist| playlist.name == "Road"));
        assert!(loaded.iter().any(|playlist| playlist.name == "Sleep"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_duplicates_playlist_without_losing_tracks() {
        let dir = test_dir("duplicate-copy");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Road".to_string()).unwrap();
        store
            .add_songs_to_index(
                0,
                &[Song {
                    title: "Keep Me".to_string(),
                    artist: "Artist".to_string(),
                    album_name: "Album".to_string(),
                    stream_service: Some("Qobuz".to_string()),
                    stream_track_id: Some("track-1".to_string()),
                    ..Default::default()
                }],
            )
            .unwrap();

        let result = store.duplicate_at(0, "  Road Copy  ".to_string()).unwrap();

        assert_eq!(result, Some(("Road".to_string(), "Road Copy".to_string())));
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 2);
        let copy = loaded
            .iter()
            .find(|playlist| playlist.name == "Road Copy")
            .expect("copied playlist should exist");
        assert_eq!(copy.tracks.len(), 1);
        assert_eq!(copy.tracks[0].title, "Keep Me");
        assert_eq!(copy.tracks[0].artist, "Artist");
        assert_eq!(copy.tracks[0].album_name, "Album");
        assert_eq!(copy.tracks[0].stream_service.as_deref(), Some("Qobuz"));
        assert_eq!(copy.tracks[0].stream_track_id.as_deref(), Some("track-1"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_rejects_duplicate_playlist_copy_name_without_overwriting() {
        let dir = test_dir("duplicate-copy-name");
        let store = PlaylistStore::with_dir(dir.clone());
        store.create("Road".to_string()).unwrap();
        store.create("Road Copy".to_string()).unwrap();

        let err = store.duplicate_at(0, "road copy".to_string()).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|playlist| playlist.name == "Road"));
        assert!(loaded.iter().any(|playlist| playlist.name == "Road Copy"));

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

        assert_eq!(source.get_albums(), vec!["Mix (1 track)"]);
        let path = source.get_album_path(0).unwrap();
        let songs = source.get_songs_from_album(path);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].title, "Track");

        let _ = fs::remove_dir_all(dir);
    }
}
