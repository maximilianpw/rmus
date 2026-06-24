use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    config::{config_path, Config},
    history::HistoryStore,
    local_cache::LocalTrackCache,
    playlist::{PlaylistExportSummary, PlaylistImportSummary, PlaylistStore},
    queue::QueueStore,
    sources::{
        local::{LocalFiles, LocalLibraryStats},
        song::Song,
    },
    utils::{expand_home_path, track_count_label},
};

const DEFAULT_LOCAL_SEARCH_LIMIT: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Help,
    Version,
    Doctor,
    Paths,
    ListSources,
    ListPlaylists,
    LocalStats,
    SearchLocal {
        query: String,
        limit: usize,
    },
    ScanLocal {
        name: Option<String>,
    },
    AddSource {
        name: String,
        path: PathBuf,
        scan: bool,
    },
    RemoveSource {
        name: String,
    },
    ShowPlaylist {
        name: String,
    },
    DeletePlaylist {
        name: String,
    },
    ImportPlaylist {
        path: PathBuf,
        name: Option<String>,
    },
    ExportPlaylist {
        name: String,
        path: PathBuf,
    },
    ClearCache,
}

pub fn parse_args<I, S>(args: I) -> Result<CliAction, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter();
    let _program = args.next();

    let Some(first) = args.next().map(Into::into) else {
        return Ok(CliAction::Run);
    };
    let args = args.map(Into::into);

    match first.as_str() {
        "-h" | "--help" => no_more_args(args, CliAction::Help, &first),
        "-V" | "--version" => no_more_args(args, CliAction::Version, &first),
        "doctor" => no_more_args(args, CliAction::Doctor, &first),
        "paths" => no_more_args(args, CliAction::Paths, &first),
        "list-sources" => no_more_args(args, CliAction::ListSources, &first),
        "list-playlists" => no_more_args(args, CliAction::ListPlaylists, &first),
        "local-stats" => no_more_args(args, CliAction::LocalStats, &first),
        "search-local" => parse_search_local_args(args),
        "scan-local" => parse_scan_local_args(args),
        "add-source" => parse_add_source_args(args),
        "remove-source" => parse_remove_source_args(args),
        "show-playlist" => parse_show_playlist_args(args),
        "delete-playlist" => parse_delete_playlist_args(args),
        "import-playlist" => parse_import_playlist_args(args),
        "export-playlist" => parse_export_playlist_args(args),
        "clear-cache" => no_more_args(args, CliAction::ClearCache, &first),
        _ => Err(format!("unknown argument '{first}'\n\n{}", help_text())),
    }
}

fn no_more_args<I>(mut args: I, action: CliAction, first: &str) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after '{first}'\n\n{}",
            help_text()
        ));
    }
    Ok(action)
}

fn parse_add_source_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(name) = args.next() else {
        return Err(format!("missing name for add-source\n\n{}", help_text()));
    };
    let Some(path) = args.next() else {
        return Err(format!("missing path for add-source\n\n{}", help_text()));
    };
    let scan = match args.next() {
        Some(flag) if flag == "--scan" => true,
        Some(_) => {
            return Err(format!(
                "unexpected argument after source path\n\n{}",
                help_text()
            ));
        }
        None => false,
    };
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after --scan\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::AddSource {
        name,
        path: PathBuf::from(path),
        scan,
    })
}

fn parse_remove_source_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(name) = args.next() else {
        return Err(format!("missing name for remove-source\n\n{}", help_text()));
    };
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after source name\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::RemoveSource { name })
}

fn parse_scan_local_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let name = args.next();
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after source name\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::ScanLocal { name })
}

fn parse_search_local_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(query) = args.next() else {
        return Err(format!("missing query for search-local\n\n{}", help_text()));
    };

    let mut limit = DEFAULT_LOCAL_SEARCH_LIMIT;
    match args.next() {
        Some(flag) if flag == "--limit" => {
            let Some(value) = args.next() else {
                return Err(format!("missing value for --limit\n\n{}", help_text()));
            };
            limit = parse_positive_limit(&value)?;
        }
        Some(_) => {
            return Err(format!(
                "unexpected argument after search query\n\n{}",
                help_text()
            ));
        }
        None => {}
    }
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after --limit value\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::SearchLocal { query, limit })
}

fn parse_positive_limit(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| format!("--limit must be a positive integer: {value}"))?;
    if limit == 0 {
        return Err("--limit must be greater than 0".to_string());
    }
    Ok(limit)
}

fn parse_show_playlist_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(name) = args.next() else {
        return Err(format!(
            "missing playlist name for show-playlist\n\n{}",
            help_text()
        ));
    };
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after playlist name\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::ShowPlaylist { name })
}

fn parse_delete_playlist_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(name) = args.next() else {
        return Err(format!(
            "missing playlist name for delete-playlist\n\n{}",
            help_text()
        ));
    };
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after playlist name\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::DeletePlaylist { name })
}

fn parse_import_playlist_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(path) = args.next() else {
        return Err(format!(
            "missing path for import-playlist\n\n{}",
            help_text()
        ));
    };
    let name = args.next();
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after playlist name\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::ImportPlaylist {
        path: PathBuf::from(path),
        name,
    })
}

fn parse_export_playlist_args<I>(mut args: I) -> Result<CliAction, String>
where
    I: Iterator<Item = String>,
{
    let Some(name) = args.next() else {
        return Err(format!(
            "missing playlist name for export-playlist\n\n{}",
            help_text()
        ));
    };
    let Some(path) = args.next() else {
        return Err(format!(
            "missing path for export-playlist\n\n{}",
            help_text()
        ));
    };
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after export path\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::ExportPlaylist {
        name,
        path: PathBuf::from(path),
    })
}

pub fn version_text() -> String {
    format!("rmus {}", env!("CARGO_PKG_VERSION"))
}

pub fn help_text() -> &'static str {
    concat!(
        "rmus - keyboard-driven terminal music player\n",
        "\n",
        "Usage:\n",
        "  rmus [OPTIONS] [COMMAND]\n",
        "\n",
        "Commands:\n",
        "  doctor          Check runtime dependencies and app paths\n",
        "  paths           Print app storage paths\n",
        "  list-sources    Print configured local music folders\n",
        "  list-playlists  Print saved playlists and track counts\n",
        "  local-stats     Count configured local sources, albums, and tracks\n",
        "  search-local <QUERY> [--limit N]\n",
        "                  Search configured local tracks without launching the TUI\n",
        "  scan-local [NAME]\n",
        "                  Scan all, or a named local source, into the metadata cache\n",
        "  add-source <NAME> <PATH> [--scan]\n",
        "                  Add a local music folder to config, optionally warming the cache\n",
        "  remove-source <NAME>\n",
        "                  Remove a local music folder from config\n",
        "  show-playlist <NAME>\n",
        "                  Print saved tracks in a playlist\n",
        "  delete-playlist <NAME>\n",
        "                  Delete a saved playlist\n",
        "  import-playlist <PATH> [NAME]\n",
        "                  Import a local .m3u/.m3u8 playlist\n",
        "  export-playlist <NAME> <PATH>\n",
        "                  Export a playlist to .m3u8\n",
        "  clear-cache     Remove cached local discovery and metadata\n",
        "\n",
        "Options:\n",
        "  -h, --help       Print help\n",
        "  -V, --version    Print version\n"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSourceListEntry {
    pub name: String,
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSourceListSummary {
    pub sources: Vec<LocalSourceListEntry>,
}

impl LocalSourceListSummary {
    pub fn message(&self) -> String {
        if self.sources.is_empty() {
            return "No local sources configured; add folders in Settings first".to_string();
        }

        let mut text = format!("Local sources ({}):\n", self.sources.len());
        for source in &self.sources {
            let status = if source.exists { "ok" } else { "missing" };
            text.push_str(&format!(
                "- {}: {} [{}]\n",
                source.name,
                source.path.to_string_lossy(),
                status
            ));
        }
        text
    }
}

pub fn list_sources() -> LocalSourceListSummary {
    list_sources_from_config(Config::load())
}

fn list_sources_from_config(config: Config) -> LocalSourceListSummary {
    LocalSourceListSummary {
        sources: config
            .get_local_sources()
            .into_iter()
            .map(|source| LocalSourceListEntry {
                exists: source.path.is_dir(),
                name: source.name,
                path: source.path,
            })
            .collect(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistListSummary {
    pub playlists: Vec<crate::playlist::PlaylistSummary>,
}

impl PlaylistListSummary {
    pub fn message(&self) -> String {
        if self.playlists.is_empty() {
            return "No playlists found; create one in the Playlists tab or import with `rmus import-playlist`".to_string();
        }

        let mut text = format!("Playlists ({}):\n", self.playlists.len());
        for playlist in &self.playlists {
            text.push_str(&format!("- {}\n", playlist.display_title()));
        }
        text
    }
}

pub fn list_playlists() -> PlaylistListSummary {
    list_playlists_with_store(PlaylistStore::default())
}

fn list_playlists_with_store(store: PlaylistStore) -> PlaylistListSummary {
    PlaylistListSummary {
        playlists: store.summaries(),
    }
}

#[derive(Debug, Clone)]
pub struct PlaylistDetailSummary {
    pub name: String,
    pub tracks: Vec<crate::playlist::PlaylistTrack>,
}

impl PlaylistDetailSummary {
    pub fn message(&self) -> String {
        let mut text = format!(
            "Playlist '{}' ({})\n",
            self.name,
            track_count_label(self.tracks.len())
        );

        if self.tracks.is_empty() {
            text.push_str("No tracks saved.\n");
            return text;
        }

        for (index, track) in self.tracks.iter().enumerate() {
            text.push_str(&format!(
                "{}. {}\n",
                index + 1,
                playlist_track_detail(track)
            ));
        }
        text
    }
}

pub fn show_playlist(name: &str) -> Result<PlaylistDetailSummary, String> {
    show_playlist_with_store(PlaylistStore::default(), name)
}

fn show_playlist_with_store(
    store: PlaylistStore,
    name: &str,
) -> Result<PlaylistDetailSummary, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name is required".to_string());
    }

    let Some(playlist) = store
        .load_all()
        .into_iter()
        .find(|playlist| playlist.name.eq_ignore_ascii_case(name))
    else {
        return Err(format!("Playlist '{name}' not found"));
    };

    Ok(PlaylistDetailSummary {
        name: playlist.name,
        tracks: playlist.tracks,
    })
}

fn playlist_track_detail(track: &crate::playlist::PlaylistTrack) -> String {
    let mut detail = playlist_track_title(track);
    if let Some(album) = nonblank(&track.album_name) {
        detail.push_str(&format!(" ({album})"));
    }
    detail.push_str(&format!(" [{}]", playlist_track_source_label(track)));

    if let Some(path) = track.path.as_deref().and_then(nonblank) {
        detail.push(' ');
        detail.push_str(path);
    }

    detail
}

fn playlist_track_title(track: &crate::playlist::PlaylistTrack) -> String {
    let title = nonblank(&track.title);
    let artist = nonblank(&track.artist);
    match (artist, title) {
        (Some(artist), Some(title)) => format!("{artist} - {title}"),
        (None, Some(title)) => title.to_string(),
        (Some(artist), None) => artist.to_string(),
        (None, None) => track
            .path
            .as_deref()
            .and_then(nonblank)
            .and_then(|path| {
                Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.trim().is_empty())
            })
            .or_else(|| track.stream_track_id.as_deref().and_then(nonblank))
            .unwrap_or("Unknown Track")
            .to_string(),
    }
}

fn playlist_track_source_label(track: &crate::playlist::PlaylistTrack) -> String {
    if track.path.as_deref().and_then(nonblank).is_some() {
        return "local".to_string();
    }

    let service = track.stream_service.as_deref().and_then(nonblank);
    let track_id = track.stream_track_id.as_deref().and_then(nonblank);
    match (service, track_id) {
        (Some(service), Some(track_id)) => format!("{service}: {track_id}"),
        (Some(service), None) => service.to_string(),
        (None, Some(track_id)) => format!("stream: {track_id}"),
        (None, None) => "saved".to_string(),
    }
}

fn nonblank(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistDeleteSummary {
    pub name: String,
    pub track_count: usize,
}

impl PlaylistDeleteSummary {
    pub fn message(&self) -> String {
        format!(
            "Deleted playlist '{}' ({})",
            self.name,
            track_count_label(self.track_count)
        )
    }
}

pub fn delete_playlist(name: &str) -> Result<PlaylistDeleteSummary, String> {
    delete_playlist_with_store(PlaylistStore::default(), name)
}

fn delete_playlist_with_store(
    store: PlaylistStore,
    name: &str,
) -> Result<PlaylistDeleteSummary, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Playlist name is required".to_string());
    }

    let playlists = store.load_all();
    let Some((index, playlist)) = playlists
        .iter()
        .enumerate()
        .find(|(_, playlist)| playlist.name.eq_ignore_ascii_case(name))
    else {
        return Err(format!("Playlist '{name}' not found"));
    };

    let summary = PlaylistDeleteSummary {
        name: playlist.name.clone(),
        track_count: playlist.tracks.len(),
    };
    store
        .delete_at(index)
        .map_err(|error| format!("failed to delete playlist '{}': {error}", summary.name))?;

    Ok(summary)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSourceRemoveSummary {
    pub name: String,
    pub path: PathBuf,
    pub source_count: usize,
}

impl LocalSourceRemoveSummary {
    pub fn message(&self) -> String {
        format!(
            "Removed local source '{}' at {}; {} configured {}",
            self.name,
            self.path.to_string_lossy(),
            self.source_count,
            plural(self.source_count, "source", "sources")
        )
    }
}

pub fn remove_source(name: &str) -> Result<LocalSourceRemoveSummary, String> {
    let mut config = Config::load();
    let summary = remove_source_from_config(&mut config, name)?;
    config
        .save()
        .map_err(|error| format!("failed to save config: {error}"))?;
    Ok(summary)
}

fn remove_source_from_config(
    config: &mut Config,
    name: &str,
) -> Result<LocalSourceRemoveSummary, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("source name is required".to_string());
    }

    let Some(index) = config
        .local
        .sources
        .iter()
        .position(|source| source.name.eq_ignore_ascii_case(name))
    else {
        return Err(format!("source not found: {name}"));
    };

    let removed = config.local.sources.remove(index);
    Ok(LocalSourceRemoveSummary {
        name: removed.name,
        path: removed.path,
        source_count: config.local.sources.len(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSourceAddSummary {
    pub name: String,
    pub path: PathBuf,
    pub source_count: usize,
}

impl LocalSourceAddSummary {
    pub fn message(&self) -> String {
        format!(
            "Added local source '{}' at {}; {} configured {}",
            self.name,
            self.path.to_string_lossy(),
            self.source_count,
            plural(self.source_count, "source", "sources")
        )
    }
}

pub fn add_source(name: &str, path: &Path) -> Result<LocalSourceAddSummary, String> {
    let mut config = Config::load();
    let summary = add_source_to_config(&mut config, name, path)?;
    config
        .save()
        .map_err(|error| format!("failed to save config: {error}"))?;
    Ok(summary)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSourceAddAndScanSummary {
    pub add: LocalSourceAddSummary,
    pub scan: LocalScanSummary,
}

impl LocalSourceAddAndScanSummary {
    pub fn message(&self) -> String {
        format!("{}\n{}", self.add.message(), self.scan.message())
    }
}

pub fn add_source_and_scan(
    name: &str,
    path: &Path,
) -> Result<LocalSourceAddAndScanSummary, String> {
    let mut config = Config::load();
    let add = add_source_to_config(&mut config, name, path)?;
    config
        .save()
        .map_err(|error| format!("failed to save config: {error}"))?;
    let scan = scan_local_with_cache_path(config, Some(&add.name), LocalTrackCache::default_path())
        .map_err(|error| format!("failed to scan local source '{}': {error}", add.name))?;
    Ok(LocalSourceAddAndScanSummary { add, scan })
}

#[cfg(test)]
fn add_source_and_scan_with_config_and_cache_path(
    config: &mut Config,
    name: &str,
    path: &Path,
    cache_path: PathBuf,
) -> Result<LocalSourceAddAndScanSummary, String> {
    let add = add_source_to_config(config, name, path)?;
    let scan = scan_local_with_cache_path(config.clone(), Some(&add.name), cache_path)
        .map_err(|error| format!("failed to scan local source '{}': {error}", add.name))?;

    Ok(LocalSourceAddAndScanSummary { add, scan })
}

fn add_source_to_config(
    config: &mut Config,
    name: &str,
    path: &Path,
) -> Result<LocalSourceAddSummary, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("source name is required".to_string());
    }

    let path = expand_home_path(path);
    if !path.is_dir() {
        return Err(format!(
            "source path must be an existing directory: {}",
            path.to_string_lossy()
        ));
    }
    let path = path.canonicalize().unwrap_or(path);

    if config
        .local
        .sources
        .iter()
        .any(|source| source.name.eq_ignore_ascii_case(name))
    {
        return Err(format!("source name already exists: {name}"));
    }

    if config.local.sources.iter().any(|source| {
        source
            .path
            .canonicalize()
            .map_or(source.path == path, |existing| existing == path)
    }) {
        return Err(format!(
            "source path already exists: {}",
            path.to_string_lossy()
        ));
    }

    config.add_local_source(name.to_string(), path.clone());
    Ok(LocalSourceAddSummary {
        name: name.to_string(),
        path,
        source_count: config.local.sources.len(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalStatsSummary {
    pub source_count: usize,
    pub missing_source_count: usize,
    pub album_count: usize,
    pub album_discovery_complete: bool,
    pub track_count: usize,
    pub cache_path: PathBuf,
    pub cache_exists: bool,
}

impl LocalStatsSummary {
    pub fn message(&self) -> String {
        if self.source_count == 0 {
            return "No local sources configured; add folders in Settings first".to_string();
        }

        let discovery_note = if self.album_discovery_complete {
            "complete"
        } else {
            "partial; run `rmus scan-local` to warm the full cache"
        };
        let cache_state = if self.cache_exists {
            "present"
        } else {
            "missing"
        };

        format!(
            "Local library: {} configured {}, {} missing, {} discovered {}, {} playable {}; album discovery: {}; cache: {} ({})",
            self.source_count,
            plural(self.source_count, "source", "sources"),
            self.missing_source_count,
            self.album_count,
            plural(self.album_count, "album", "albums"),
            self.track_count,
            plural(self.track_count, "track", "tracks"),
            discovery_note,
            self.cache_path.to_string_lossy(),
            cache_state
        )
    }
}

pub fn local_stats() -> LocalStatsSummary {
    local_stats_with_config(Config::load())
}

fn local_stats_with_config(config: Config) -> LocalStatsSummary {
    local_stats_with_cache_path(config, LocalTrackCache::default_path())
}

fn local_stats_with_cache_path(config: Config, cache_path: PathBuf) -> LocalStatsSummary {
    let sources = config.get_local_sources();
    let missing_source_count = sources
        .iter()
        .filter(|source| !source.path.is_dir())
        .count();
    let stats = if sources.is_empty() {
        LocalLibraryStats {
            album_count: 0,
            track_count: 0,
            album_discovery_complete: true,
        }
    } else {
        LocalFiles::library_stats_with_cache_path(&sources, cache_path.clone())
    };
    let cache_exists = cache_path.exists();

    LocalStatsSummary {
        source_count: sources.len(),
        missing_source_count,
        album_count: stats.album_count,
        album_discovery_complete: stats.album_discovery_complete,
        track_count: stats.track_count,
        cache_path,
        cache_exists,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSearchMatch {
    pub source_name: String,
    pub title: String,
    pub artist: String,
    pub album_name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSearchSummary {
    pub query: String,
    pub source_count: usize,
    pub missing_source_count: usize,
    pub match_count: usize,
    pub limit: usize,
    pub matches: Vec<LocalSearchMatch>,
}

impl LocalSearchSummary {
    pub fn message(&self) -> String {
        if self.source_count == 0 {
            return "No local sources configured; add folders in Settings first".to_string();
        }

        let showing = self.matches.len();
        let mut text = format!(
            "Local search '{}': {} {}, showing {}, {} configured {}, {} missing\n",
            self.query,
            self.match_count,
            plural(self.match_count, "match", "matches"),
            showing,
            self.source_count,
            plural(self.source_count, "source", "sources"),
            self.missing_source_count
        );

        if self.match_count == 0 {
            text.push_str("No matching local tracks.\n");
            return text;
        }

        for (index, matched) in self.matches.iter().enumerate() {
            text.push_str(&format!(
                "{}. {}\n",
                index + 1,
                local_search_match_detail(matched)
            ));
        }

        if self.match_count > self.matches.len() {
            text.push_str(&format!(
                "... {} more {}; rerun with --limit {} to show more\n",
                self.match_count - self.matches.len(),
                plural(self.match_count - self.matches.len(), "match", "matches"),
                self.match_count
            ));
        }

        text
    }
}

pub fn search_local(query: &str, limit: usize) -> Result<LocalSearchSummary, String> {
    search_local_with_config_and_cache_path(
        Config::load(),
        query,
        limit,
        LocalTrackCache::default_path(),
    )
}

fn search_local_with_config_and_cache_path(
    config: Config,
    query: &str,
    limit: usize,
    cache_path: PathBuf,
) -> Result<LocalSearchSummary, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("search query is required".to_string());
    }
    if limit == 0 {
        return Err("--limit must be greater than 0".to_string());
    }

    let sources = config.get_local_sources();
    let source_count = sources.len();
    let missing_source_count = sources
        .iter()
        .filter(|source| !source.path.is_dir())
        .count();
    let query_lower = query.to_lowercase();
    let mut seen_paths = HashSet::new();
    let mut matches = Vec::new();
    let mut match_count = 0;

    for source in sources.iter().filter(|source| source.path.is_dir()) {
        let songs = LocalFiles::songs_from_path_using_cached_metadata_with_cache_path(
            source.path.clone(),
            cache_path.clone(),
        );
        for song in songs {
            if !seen_paths.insert(song.path.clone()) {
                continue;
            }
            if !song_matches_local_query(&song, &query_lower) {
                continue;
            }

            match_count += 1;
            if matches.len() < limit {
                matches.push(LocalSearchMatch::from_song(&source.name, song));
            }
        }
    }

    Ok(LocalSearchSummary {
        query: query.to_string(),
        source_count,
        missing_source_count,
        match_count,
        limit,
        matches,
    })
}

impl LocalSearchMatch {
    fn from_song(source_name: &str, song: Song) -> Self {
        Self {
            source_name: source_name.to_string(),
            title: song.title,
            artist: song.artist,
            album_name: song.album_name,
            path: song.path,
        }
    }
}

fn song_matches_local_query(song: &Song, query_lower: &str) -> bool {
    song.title.to_lowercase().contains(query_lower)
        || song.artist.to_lowercase().contains(query_lower)
        || song.album_name.to_lowercase().contains(query_lower)
        || song
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default()
            .contains(query_lower)
        || song
            .path
            .to_string_lossy()
            .to_lowercase()
            .contains(query_lower)
}

fn local_search_match_detail(matched: &LocalSearchMatch) -> String {
    let mut detail = local_search_match_title(matched);
    if let Some(album) = nonblank(&matched.album_name) {
        detail.push_str(&format!(" ({album})"));
    }
    detail.push_str(&format!(
        " [{}] {}",
        matched.source_name,
        matched.path.to_string_lossy()
    ));
    detail
}

fn local_search_match_title(matched: &LocalSearchMatch) -> String {
    let title = nonblank(&matched.title);
    let artist = nonblank(&matched.artist);
    match (artist, title) {
        (Some(artist), Some(title)) => format!("{artist} - {title}"),
        (None, Some(title)) => title.to_string(),
        (Some(artist), None) => artist.to_string(),
        (None, None) => matched
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("Unknown Track")
            .to_string(),
    }
}

pub fn import_playlist(
    path: &Path,
    name: Option<&str>,
) -> Result<PlaylistImportSummary, std::io::Error> {
    PlaylistStore::default().import_m3u(path, name)
}

pub fn export_playlist(name: &str, path: &Path) -> Result<PlaylistExportSummary, std::io::Error> {
    PlaylistStore::default().export_m3u(name, path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheClearSummary {
    pub path: PathBuf,
    pub removed: bool,
}

impl CacheClearSummary {
    pub fn message(&self) -> String {
        let path = self.path.to_string_lossy();
        if self.removed {
            format!("Removed local cache at {path}")
        } else {
            format!("Local cache already absent at {path}")
        }
    }
}

pub fn clear_cache() -> Result<CacheClearSummary, std::io::Error> {
    clear_cache_at(&LocalTrackCache::default_path())
}

fn clear_cache_at(path: &Path) -> Result<CacheClearSummary, std::io::Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(CacheClearSummary {
            path: path.to_path_buf(),
            removed: true,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(CacheClearSummary {
            path: path.to_path_buf(),
            removed: false,
        }),
        Err(error) => Err(error),
    }
}

pub fn paths_text() -> String {
    paths_text_with_options(DoctorOptions::current())
}

fn paths_text_with_options(options: DoctorOptions) -> String {
    format!(
        "\
rmus paths
config: {}
playlists: {}
history: {}
queue: {}
local cache: {}
",
        options.config_path.to_string_lossy(),
        options.playlists_dir.to_string_lossy(),
        options.history_path.to_string_lossy(),
        options.queue_path.to_string_lossy(),
        options.local_cache_path.to_string_lossy()
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalScanSummary {
    pub source_name: Option<String>,
    pub source_count: usize,
    pub track_count: usize,
    pub cache_path: PathBuf,
}

impl LocalScanSummary {
    pub fn message(&self) -> String {
        if self.source_count == 0 {
            return "No local sources configured; add folders in Settings first".to_string();
        }

        let source_label = if let Some(name) = &self.source_name {
            format!("local source '{name}'")
        } else {
            format!(
                "{} local {}",
                self.source_count,
                plural(self.source_count, "source", "sources")
            )
        };

        format!(
            "Scanned {} local {} from {}; cache: {}",
            self.track_count,
            plural(self.track_count, "track", "tracks"),
            source_label,
            self.cache_path.to_string_lossy()
        )
    }
}

#[derive(Debug)]
pub enum LocalScanError {
    SourceNameRequired,
    SourceNotFound(String),
    Io(std::io::Error),
}

impl std::fmt::Display for LocalScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceNameRequired => write!(f, "source name is required"),
            Self::SourceNotFound(name) => write!(f, "source not found: {name}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for LocalScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::SourceNameRequired | Self::SourceNotFound(_) => None,
        }
    }
}

impl From<std::io::Error> for LocalScanError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn scan_local(source_name: Option<&str>) -> Result<LocalScanSummary, LocalScanError> {
    scan_local_with_config(Config::load(), source_name)
}

fn scan_local_with_config(
    config: Config,
    source_name: Option<&str>,
) -> Result<LocalScanSummary, LocalScanError> {
    scan_local_with_cache_path(config, source_name, LocalTrackCache::default_path())
}

fn scan_local_with_cache_path(
    config: Config,
    source_name: Option<&str>,
    cache_path: PathBuf,
) -> Result<LocalScanSummary, LocalScanError> {
    let sources = config.get_local_sources();
    let (sources, selected_name) = select_scan_sources(sources, source_name)?;
    let track_count = if sources.is_empty() {
        0
    } else {
        LocalFiles::scan_sources_with_cache_path(&sources, cache_path.clone())?
    };

    Ok(LocalScanSummary {
        source_name: selected_name,
        source_count: sources.len(),
        track_count,
        cache_path,
    })
}

fn select_scan_sources(
    sources: Vec<crate::config::LocalSource>,
    source_name: Option<&str>,
) -> Result<(Vec<crate::config::LocalSource>, Option<String>), LocalScanError> {
    let Some(name) = source_name.map(str::trim) else {
        return Ok((sources, None));
    };
    if name.is_empty() {
        return Err(LocalScanError::SourceNameRequired);
    }

    let Some(source) = sources
        .into_iter()
        .find(|source| source.name.eq_ignore_ascii_case(name))
    else {
        return Err(LocalScanError::SourceNotFound(name.to_string()));
    };

    let selected_name = source.name.clone();
    Ok((vec![source], Some(selected_name)))
}

fn plural(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorCheck {
    status: DoctorStatus,
    name: &'static str,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorStatus {
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
struct DoctorOptions {
    path_env: Option<OsString>,
    config_path: PathBuf,
    playlists_dir: PathBuf,
    history_path: PathBuf,
    queue_path: PathBuf,
    local_cache_path: PathBuf,
}

impl DoctorOptions {
    pub fn current() -> Self {
        Self {
            path_env: env::var_os("PATH"),
            config_path: config_path(),
            playlists_dir: PlaylistStore::default().dir().to_path_buf(),
            history_path: HistoryStore::default().path().to_path_buf(),
            queue_path: QueueStore::default().path().to_path_buf(),
            local_cache_path: LocalTrackCache::default_path(),
        }
    }
}

impl DoctorReport {
    pub fn exit_code(&self) -> i32 {
        if self
            .checks
            .iter()
            .any(|check| check.status == DoctorStatus::Error)
        {
            1
        } else {
            0
        }
    }

    pub fn to_text(&self) -> String {
        let mut text = String::from("rmus doctor\n");
        for check in &self.checks {
            text.push_str(&format!(
                "[{}] {}: {}\n",
                check.status.label(),
                check.name,
                check.detail
            ));
        }
        text
    }
}

impl DoctorStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

pub fn doctor_report() -> DoctorReport {
    doctor_report_with_options(DoctorOptions::current())
}

fn doctor_report_with_options(options: DoctorOptions) -> DoctorReport {
    let mut checks = vec![DoctorCheck {
        status: DoctorStatus::Ok,
        name: "version",
        detail: version_text(),
    }];

    match find_executable_in_path("mpv", options.path_env.as_deref()) {
        Some(path) => checks.push(DoctorCheck {
            status: DoctorStatus::Ok,
            name: "mpv",
            detail: path.to_string_lossy().into_owned(),
        }),
        None => checks.push(DoctorCheck {
            status: DoctorStatus::Error,
            name: "mpv",
            detail: "not found in PATH; install mpv and try again".to_string(),
        }),
    }

    checks.extend(storage_checks(&options));

    DoctorReport { checks }
}

fn storage_checks(options: &DoctorOptions) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let config_exists = options.config_path.exists();
    checks.push(path_check(
        "config",
        &options.config_path,
        config_exists,
        "missing; it will be created on first launch",
    ));
    checks.push(path_check(
        "playlists",
        &options.playlists_dir,
        options.playlists_dir.exists(),
        "missing; playlists will be created when saved",
    ));
    checks.push(path_check(
        "history",
        &options.history_path,
        options.history_path.exists(),
        "missing; playback history will be created after playing tracks",
    ));
    checks.push(path_check(
        "queue",
        &options.queue_path,
        options.queue_path.exists(),
        "missing; playback queue will be created after queue changes",
    ));
    checks.push(path_check(
        "local cache",
        &options.local_cache_path,
        options.local_cache_path.exists(),
        "missing; local track metadata will be cached after browsing local music",
    ));

    if config_exists {
        checks.push(local_source_check(&options.config_path));
    } else {
        checks.push(DoctorCheck {
            status: DoctorStatus::Warn,
            name: "local sources",
            detail: "config missing; no local sources configured yet".to_string(),
        });
    }

    checks
}

fn path_check(
    name: &'static str,
    path: &Path,
    exists: bool,
    missing_detail: &'static str,
) -> DoctorCheck {
    if exists {
        DoctorCheck {
            status: DoctorStatus::Ok,
            name,
            detail: path.to_string_lossy().into_owned(),
        }
    } else {
        DoctorCheck {
            status: DoctorStatus::Warn,
            name,
            detail: format!("{} ({missing_detail})", path.to_string_lossy()),
        }
    }
}

fn local_source_check(config_path: &Path) -> DoctorCheck {
    let config = match fs::read_to_string(config_path)
        .map_err(|error| error.to_string())
        .and_then(|content| toml::from_str::<Config>(&content).map_err(|error| error.to_string()))
    {
        Ok(config) => config,
        Err(error) => {
            return DoctorCheck {
                status: DoctorStatus::Error,
                name: "local sources",
                detail: format!("could not read config: {error}"),
            };
        }
    };

    let sources = config.get_local_sources();
    if sources.is_empty() {
        return DoctorCheck {
            status: DoctorStatus::Warn,
            name: "local sources",
            detail: "none configured".to_string(),
        };
    }

    let missing: Vec<_> = sources
        .iter()
        .filter(|source| !source.path.is_dir())
        .map(|source| format!("{} ({})", source.name, source.path.to_string_lossy()))
        .collect();

    if missing.is_empty() {
        DoctorCheck {
            status: DoctorStatus::Ok,
            name: "local sources",
            detail: format!("{} configured", sources.len()),
        }
    } else {
        DoctorCheck {
            status: DoctorStatus::Warn,
            name: "local sources",
            detail: format!("{} missing: {}", missing.len(), missing.join(", ")),
        }
    }
}

fn find_executable_in_path(name: &str, path_env: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path_env = path_env?;
    for dir in env::split_paths(path_env) {
        for candidate in executable_candidates(&dir, name) {
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    let base = dir.join(name);
    if Path::new(name).extension().is_some() {
        return vec![base];
    }

    let mut candidates = vec![base.clone()];
    if cfg!(windows) {
        let path_ext = env::var_os("PATHEXT")
            .map(|value| {
                value
                    .to_string_lossy()
                    .split(';')
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                vec![
                    ".COM".to_string(),
                    ".EXE".to_string(),
                    ".BAT".to_string(),
                    ".CMD".to_string(),
                ]
            });

        candidates.extend(
            path_ext
                .iter()
                .map(|extension| dir.join(format!("{name}{extension}"))),
        );
    }

    candidates
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_source_and_scan_with_config_and_cache_path, add_source_to_config, clear_cache_at,
        delete_playlist_with_store, doctor_report_with_options, list_playlists_with_store,
        list_sources_from_config, local_stats_with_cache_path, parse_args, paths_text_with_options,
        remove_source_from_config, scan_local_with_cache_path,
        search_local_with_config_and_cache_path, show_playlist_with_store, CliAction,
        DoctorOptions, DEFAULT_LOCAL_SEARCH_LIMIT,
    };
    use std::{
        env, fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("rmus-cli-{name}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_mpv_dir() -> PathBuf {
        let dir = test_dir("fake-mpv");
        let name = if cfg!(windows) { "mpv.exe" } else { "mpv" };
        let path = dir.join(name);
        fs::write(&path, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        dir
    }

    fn toml_string(value: &str) -> String {
        toml::Value::String(value.to_string()).to_string()
    }

    fn write_cached_track(
        cache_path: &std::path::Path,
        track_path: &std::path::Path,
        title: &str,
        artist: &str,
        album_name: &str,
    ) {
        let metadata = fs::metadata(track_path).unwrap();
        let modified = metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        fs::write(
            cache_path,
            format!(
                "\
[[tracks]]
path = {}
len = {}
modified_secs = {}
modified_nanos = {}
title = {}
artist = {}
album_name = {}
",
                toml_string(&track_path.to_string_lossy()),
                metadata.len(),
                modified.as_secs(),
                modified.subsec_nanos(),
                toml_string(title),
                toml_string(artist),
                toml_string(album_name)
            ),
        )
        .unwrap();
    }

    fn doctor_options(path_env: Option<std::ffi::OsString>) -> DoctorOptions {
        let dir = test_dir("doctor-options");
        DoctorOptions {
            path_env,
            config_path: dir.join("config.toml"),
            playlists_dir: dir.join("playlists"),
            history_path: dir.join("history.toml"),
            queue_path: dir.join("queue.toml"),
            local_cache_path: dir.join("local-cache.toml"),
        }
    }

    #[test]
    fn no_args_runs_tui() {
        assert_eq!(parse_args(["rmus"]), Ok(CliAction::Run));
    }

    #[test]
    fn help_flags_print_help() {
        assert_eq!(parse_args(["rmus", "--help"]), Ok(CliAction::Help));
        assert_eq!(parse_args(["rmus", "-h"]), Ok(CliAction::Help));
    }

    #[test]
    fn version_flags_print_version() {
        assert_eq!(parse_args(["rmus", "--version"]), Ok(CliAction::Version));
        assert_eq!(parse_args(["rmus", "-V"]), Ok(CliAction::Version));
    }

    #[test]
    fn doctor_command_runs_diagnostics() {
        assert_eq!(parse_args(["rmus", "doctor"]), Ok(CliAction::Doctor));
    }

    #[test]
    fn paths_command_prints_storage_paths() {
        assert_eq!(parse_args(["rmus", "paths"]), Ok(CliAction::Paths));

        let dir = test_dir("paths");
        let text = paths_text_with_options(DoctorOptions {
            path_env: None,
            config_path: dir.join("config.toml"),
            playlists_dir: dir.join("playlists"),
            history_path: dir.join("history.toml"),
            queue_path: dir.join("queue.toml"),
            local_cache_path: dir.join("local-cache.toml"),
        });

        assert!(text.contains("rmus paths"));
        assert!(text.contains(&format!(
            "config: {}",
            dir.join("config.toml").to_string_lossy()
        )));
        assert!(text.contains(&format!(
            "playlists: {}",
            dir.join("playlists").to_string_lossy()
        )));
        assert!(text.contains(&format!(
            "history: {}",
            dir.join("history.toml").to_string_lossy()
        )));
        assert!(text.contains(&format!(
            "queue: {}",
            dir.join("queue.toml").to_string_lossy()
        )));
        assert!(text.contains(&format!(
            "local cache: {}",
            dir.join("local-cache.toml").to_string_lossy()
        )));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_sources_command_prints_configured_sources() {
        assert_eq!(
            parse_args(["rmus", "list-sources"]),
            Ok(CliAction::ListSources)
        );

        let dir = test_dir("list-sources");
        let missing = dir.join("Missing");
        let summary = list_sources_from_config(crate::config::Config {
            local: crate::config::LocalConfig {
                sources: vec![
                    crate::config::LocalSource {
                        name: "Library".to_string(),
                        path: dir.clone(),
                    },
                    crate::config::LocalSource {
                        name: "Missing".to_string(),
                        path: missing.clone(),
                    },
                ],
            },
            ..crate::config::Config::default()
        });

        assert_eq!(summary.sources.len(), 2);
        assert_eq!(summary.sources[0].name, "Library");
        assert_eq!(summary.sources[0].path, dir);
        assert!(summary.sources[0].exists);
        assert_eq!(summary.sources[1].name, "Missing");
        assert_eq!(summary.sources[1].path, missing);
        assert!(!summary.sources[1].exists);
        let message = summary.message();
        assert!(message.contains("Local sources (2):"));
        assert!(message.contains("- Library:"));
        assert!(message.contains("[ok]"));
        assert!(message.contains("- Missing:"));
        assert!(message.contains("[missing]"));

        let _ = fs::remove_dir_all(summary.sources[0].path.clone());
    }

    #[test]
    fn list_sources_reports_empty_config() {
        let summary = list_sources_from_config(crate::config::Config::default());

        assert!(summary.sources.is_empty());
        assert_eq!(
            summary.message(),
            "No local sources configured; add folders in Settings first"
        );
    }

    #[test]
    fn list_playlists_command_prints_saved_playlists() {
        assert_eq!(
            parse_args(["rmus", "list-playlists"]),
            Ok(CliAction::ListPlaylists)
        );

        let dir = test_dir("list-playlists");
        let playlists_dir = dir.join("playlists");
        fs::create_dir_all(&playlists_dir).unwrap();
        fs::write(
            playlists_dir.join("Road.toml"),
            r#"
name = "Road"

[[tracks]]
title = "First"

[[tracks]]
title = "Second"
"#,
        )
        .unwrap();
        fs::write(
            playlists_dir.join("Favorites.toml"),
            r#"
name = "Favorites"

[[tracks]]
title = "Only"
"#,
        )
        .unwrap();

        let summary =
            list_playlists_with_store(crate::playlist::PlaylistStore::with_dir(playlists_dir));

        assert_eq!(summary.playlists.len(), 2);
        assert_eq!(summary.playlists[0].name, "Favorites");
        assert_eq!(summary.playlists[0].track_count, 1);
        assert_eq!(summary.playlists[1].name, "Road");
        assert_eq!(summary.playlists[1].track_count, 2);
        let message = summary.message();
        assert!(message.contains("Playlists (2):"));
        assert!(message.contains("- Favorites (1 track)"));
        assert!(message.contains("- Road (2 tracks)"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn list_playlists_reports_empty_store() {
        let dir = test_dir("list-playlists-empty");
        let summary = list_playlists_with_store(crate::playlist::PlaylistStore::with_dir(
            dir.join("missing"),
        ));

        assert!(summary.playlists.is_empty());
        assert_eq!(
            summary.message(),
            "No playlists found; create one in the Playlists tab or import with `rmus import-playlist`"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn show_playlist_command_prints_saved_tracks() {
        assert_eq!(
            parse_args(["rmus", "show-playlist", "Road Mix"]),
            Ok(CliAction::ShowPlaylist {
                name: "Road Mix".to_string()
            })
        );
        let missing_name =
            parse_args(["rmus", "show-playlist"]).expect_err("name should be required");
        assert!(missing_name.contains("missing playlist name for show-playlist"));
        let extra = parse_args(["rmus", "show-playlist", "Road Mix", "extra"])
            .expect_err("extra args should fail");
        assert!(extra.contains("unexpected argument after playlist name"));

        let dir = test_dir("show-playlist");
        let playlists_dir = dir.join("playlists");
        fs::create_dir_all(&playlists_dir).unwrap();
        fs::write(
            playlists_dir.join("Road Mix.toml"),
            r#"
name = "Road Mix"

[[tracks]]
title = "Local Song"
artist = "Local Artist"
album_name = "Local Album"
path = "/music/local.flac"

[[tracks]]
title = "Stream Song"
artist = "Stream Artist"
album_name = "Stream Album"
stream_service = "Qobuz"
stream_track_id = "qbz-1"
"#,
        )
        .unwrap();
        fs::write(
            playlists_dir.join("Empty.toml"),
            r#"
name = "Empty"
tracks = []
"#,
        )
        .unwrap();
        let store = crate::playlist::PlaylistStore::with_dir(playlists_dir);

        let summary = show_playlist_with_store(store.clone(), "road mix").unwrap();
        assert_eq!(summary.name, "Road Mix");
        assert_eq!(summary.tracks.len(), 2);
        let message = summary.message();
        assert!(message.contains("Playlist 'Road Mix' (2 tracks)"));
        assert!(message
            .contains("1. Local Artist - Local Song (Local Album) [local] /music/local.flac"));
        assert!(message.contains("2. Stream Artist - Stream Song (Stream Album) [Qobuz: qbz-1]"));

        let empty = show_playlist_with_store(store.clone(), "Empty").unwrap();
        assert_eq!(
            empty.message(),
            "Playlist 'Empty' (0 tracks)\nNo tracks saved.\n"
        );

        let blank = show_playlist_with_store(store.clone(), " ")
            .expect_err("blank names should be rejected");
        assert_eq!(blank, "Playlist name is required");
        let missing =
            show_playlist_with_store(store, "Missing").expect_err("unknown playlists should fail");
        assert_eq!(missing, "Playlist 'Missing' not found");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn delete_playlist_command_removes_saved_playlist() {
        assert_eq!(
            parse_args(["rmus", "delete-playlist", "Road Mix"]),
            Ok(CliAction::DeletePlaylist {
                name: "Road Mix".to_string()
            })
        );
        let missing_name =
            parse_args(["rmus", "delete-playlist"]).expect_err("name should be required");
        assert!(missing_name.contains("missing playlist name for delete-playlist"));
        let extra = parse_args(["rmus", "delete-playlist", "Road Mix", "extra"])
            .expect_err("extra args should fail");
        assert!(extra.contains("unexpected argument after playlist name"));

        let dir = test_dir("delete-playlist");
        let playlists_dir = dir.join("playlists");
        fs::create_dir_all(&playlists_dir).unwrap();
        fs::write(
            playlists_dir.join("Road Mix.toml"),
            r#"
name = "Road Mix"

[[tracks]]
title = "First"

[[tracks]]
title = "Second"
"#,
        )
        .unwrap();
        fs::write(
            playlists_dir.join("Keep.toml"),
            r#"
name = "Keep"
tracks = []
"#,
        )
        .unwrap();
        let store = crate::playlist::PlaylistStore::with_dir(playlists_dir.clone());

        let summary = delete_playlist_with_store(store.clone(), "road mix").unwrap();

        assert_eq!(summary.name, "Road Mix");
        assert_eq!(summary.track_count, 2);
        assert_eq!(summary.message(), "Deleted playlist 'Road Mix' (2 tracks)");
        assert!(!playlists_dir.join("Road Mix.toml").exists());
        assert!(playlists_dir.join("Keep.toml").exists());

        let blank = delete_playlist_with_store(store.clone(), " ")
            .expect_err("blank names should be rejected");
        assert_eq!(blank, "Playlist name is required");
        let missing = delete_playlist_with_store(store, "Missing")
            .expect_err("unknown playlists should fail");
        assert_eq!(missing, "Playlist 'Missing' not found");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn local_stats_command_counts_configured_library_without_warming_cache() {
        assert_eq!(
            parse_args(["rmus", "local-stats"]),
            Ok(CliAction::LocalStats)
        );

        let dir = test_dir("local-stats");
        let album = dir.join("Album");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("01 - First.flac"), "not real audio").unwrap();
        fs::write(album.join("02 - Second.opus"), "not real audio").unwrap();
        fs::write(album.join("cover.jpg"), "not audio").unwrap();
        let missing = dir.join("Missing");
        let cache_path = dir.join("local-cache.toml");

        let summary = local_stats_with_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![
                        crate::config::LocalSource {
                            name: "Library".to_string(),
                            path: dir.clone(),
                        },
                        crate::config::LocalSource {
                            name: "Missing".to_string(),
                            path: missing,
                        },
                    ],
                },
                ..crate::config::Config::default()
            },
            cache_path.clone(),
        );

        assert_eq!(summary.source_count, 2);
        assert_eq!(summary.missing_source_count, 1);
        assert_eq!(summary.album_count, 1);
        assert_eq!(summary.track_count, 2);
        assert!(summary.album_discovery_complete);
        assert!(!summary.cache_exists);
        assert!(!cache_path.exists());
        let message = summary.message();
        assert!(message.contains("2 configured sources"));
        assert!(message.contains("1 missing"));
        assert!(message.contains("1 discovered album"));
        assert!(message.contains("2 playable tracks"));
        assert!(message.contains("cache:"));
        assert!(message.contains("(missing)"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn local_stats_reports_missing_sources() {
        let dir = test_dir("local-stats-missing");
        let summary =
            local_stats_with_cache_path(crate::config::Config::default(), dir.join("cache.toml"));

        assert_eq!(summary.source_count, 0);
        assert_eq!(summary.track_count, 0);
        assert_eq!(
            summary.message(),
            "No local sources configured; add folders in Settings first"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_local_command_accepts_query_and_limit() {
        assert_eq!(
            parse_args(["rmus", "search-local", "joy"]),
            Ok(CliAction::SearchLocal {
                query: "joy".to_string(),
                limit: DEFAULT_LOCAL_SEARCH_LIMIT,
            })
        );
        assert_eq!(
            parse_args(["rmus", "search-local", "joy", "--limit", "10"]),
            Ok(CliAction::SearchLocal {
                query: "joy".to_string(),
                limit: 10,
            })
        );

        let missing_query =
            parse_args(["rmus", "search-local"]).expect_err("query should be required");
        assert!(missing_query.contains("missing query for search-local"));

        let missing_limit = parse_args(["rmus", "search-local", "joy", "--limit"])
            .expect_err("limit should require value");
        assert!(missing_limit.contains("missing value for --limit"));

        let invalid_limit = parse_args(["rmus", "search-local", "joy", "--limit", "0"])
            .expect_err("zero limit should fail");
        assert!(invalid_limit.contains("--limit must be greater than 0"));

        let unexpected = parse_args(["rmus", "search-local", "joy", "extra"])
            .expect_err("extra search args should fail");
        assert!(unexpected.contains("unexpected argument after search query"));
    }

    #[test]
    fn search_local_uses_cached_metadata_and_limits_output() {
        let dir = test_dir("search-local");
        let track = dir.join("01 - Track.flac");
        let other = dir.join("02 - Other.flac");
        fs::write(&track, "audio").unwrap();
        fs::write(&other, "audio").unwrap();
        let cache_path = dir.join("local-cache.toml");
        write_cached_track(
            &cache_path,
            &track,
            "Disorder",
            "Joy Division",
            "Unknown Pleasures",
        );

        let summary = search_local_with_config_and_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![crate::config::LocalSource {
                        name: "Library".to_string(),
                        path: dir.clone(),
                    }],
                },
                ..crate::config::Config::default()
            },
            "joy",
            1,
            cache_path,
        )
        .unwrap();

        assert_eq!(summary.source_count, 1);
        assert_eq!(summary.missing_source_count, 0);
        assert_eq!(summary.match_count, 1);
        assert_eq!(summary.matches.len(), 1);
        assert_eq!(summary.matches[0].source_name, "Library");
        assert_eq!(summary.matches[0].title, "Disorder");
        assert_eq!(summary.matches[0].artist, "Joy Division");
        assert_eq!(summary.matches[0].album_name, "Unknown Pleasures");
        assert_eq!(summary.matches[0].path, track);
        let message = summary.message();
        assert!(message.contains("Local search 'joy': 1 match, showing 1"));
        assert!(message.contains("Joy Division - Disorder (Unknown Pleasures) [Library]"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_local_matches_filename_path_and_reports_missing_sources() {
        let dir = test_dir("search-local-fallback");
        let missing = dir.join("Missing");
        let album = dir.join("Live");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("01 - Encore.flac"), "audio").unwrap();

        let summary = search_local_with_config_and_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![
                        crate::config::LocalSource {
                            name: "Shows".to_string(),
                            path: dir.clone(),
                        },
                        crate::config::LocalSource {
                            name: "Missing".to_string(),
                            path: missing,
                        },
                    ],
                },
                ..crate::config::Config::default()
            },
            "encore",
            25,
            dir.join("local-cache.toml"),
        )
        .unwrap();

        assert_eq!(summary.source_count, 2);
        assert_eq!(summary.missing_source_count, 1);
        assert_eq!(summary.match_count, 1);
        assert_eq!(summary.matches[0].title, "01 - Encore.flac");
        assert!(summary.matches[0].path.ends_with("01 - Encore.flac"));
        let message = summary.message();
        assert!(message.contains("2 configured sources, 1 missing"));
        assert!(message.contains("01 - Encore.flac [Shows]"));

        let none = search_local_with_config_and_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![crate::config::LocalSource {
                        name: "Shows".to_string(),
                        path: dir.clone(),
                    }],
                },
                ..crate::config::Config::default()
            },
            "zzzz",
            25,
            dir.join("local-cache.toml"),
        )
        .unwrap();
        assert_eq!(none.match_count, 0);
        assert!(none.message().contains("No matching local tracks."));

        let blank = search_local_with_config_and_cache_path(
            crate::config::Config::default(),
            " ",
            25,
            dir.join("local-cache.toml"),
        )
        .expect_err("blank query should fail");
        assert_eq!(blank, "search query is required");

        let empty = search_local_with_config_and_cache_path(
            crate::config::Config::default(),
            "anything",
            25,
            dir.join("local-cache.toml"),
        )
        .unwrap();
        assert_eq!(
            empty.message(),
            "No local sources configured; add folders in Settings first"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_source_command_accepts_name_and_path() {
        assert_eq!(
            parse_args(["rmus", "add-source", "Library", "/music"]),
            Ok(CliAction::AddSource {
                name: "Library".to_string(),
                path: PathBuf::from("/music"),
                scan: false,
            })
        );
        assert_eq!(
            parse_args(["rmus", "add-source", "Library", "/music", "--scan"]),
            Ok(CliAction::AddSource {
                name: "Library".to_string(),
                path: PathBuf::from("/music"),
                scan: true,
            })
        );
    }

    #[test]
    fn add_source_command_requires_name_and_path() {
        let error = parse_args(["rmus", "add-source"]).expect_err("name should be required");
        assert!(error.contains("missing name for add-source"));

        let error =
            parse_args(["rmus", "add-source", "Library"]).expect_err("path should be required");
        assert!(error.contains("missing path for add-source"));

        let error = parse_args(["rmus", "add-source", "Library", "/music", "--unknown"])
            .expect_err("unknown add-source flags should fail");
        assert!(error.contains("unexpected argument after source path"));

        let error = parse_args(["rmus", "add-source", "Library", "/music", "--scan", "extra"])
            .expect_err("extra scan args should fail");
        assert!(error.contains("unexpected argument after --scan"));
    }

    #[test]
    fn add_source_to_config_validates_and_canonicalizes_path() {
        let dir = test_dir("add-source");
        let entered = dir.join(".");
        let mut config = crate::config::Config::default();

        let summary = add_source_to_config(&mut config, " Library ", &entered).unwrap();

        assert_eq!(summary.name, "Library");
        assert_eq!(summary.path, dir.canonicalize().unwrap());
        assert_eq!(summary.source_count, 1);
        assert_eq!(config.local.sources[0].name, "Library");
        assert_eq!(config.local.sources[0].path, dir.canonicalize().unwrap());
        assert!(summary.message().contains("Added local source 'Library'"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_source_and_scan_warms_new_source_cache() {
        let dir = test_dir("add-source-scan");
        fs::write(dir.join("01 - Warmed.flac"), "").unwrap();
        let cache_path = dir.join("local-cache.toml");
        let mut config = crate::config::Config::default();

        let summary = add_source_and_scan_with_config_and_cache_path(
            &mut config,
            " Library ",
            &dir.join("."),
            cache_path.clone(),
        )
        .unwrap();

        assert_eq!(summary.add.name, "Library");
        assert_eq!(summary.add.source_count, 1);
        assert_eq!(summary.scan.source_name, Some("Library".to_string()));
        assert_eq!(summary.scan.source_count, 1);
        assert_eq!(summary.scan.track_count, 1);
        assert!(summary.message().contains("Added local source 'Library'"));
        assert!(summary.message().contains("Scanned 1 local track"));
        assert!(cache_path.exists());
        let cache = fs::read_to_string(&cache_path).unwrap();
        assert!(cache.contains("01 - Warmed.flac"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_source_to_config_rejects_duplicate_names_and_paths() {
        let dir = test_dir("add-source-duplicates");
        let mut config = crate::config::Config::default();
        add_source_to_config(&mut config, "Library", &dir).unwrap();

        let name_error = add_source_to_config(&mut config, "library", &dir.join("."))
            .expect_err("case-insensitive duplicate names should be rejected");
        assert!(name_error.contains("source name already exists"));

        let path_error = add_source_to_config(&mut config, "Other", &dir.join("."))
            .expect_err("canonical duplicate paths should be rejected");
        assert!(path_error.contains("source path already exists"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_source_to_config_rejects_missing_directories() {
        let dir = test_dir("add-source-missing");
        let missing = dir.join("Missing");
        let mut config = crate::config::Config::default();

        let error =
            add_source_to_config(&mut config, "Missing", &missing).expect_err("path should exist");

        assert!(error.contains("source path must be an existing directory"));
        assert!(config.local.sources.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_source_command_accepts_name() {
        assert_eq!(
            parse_args(["rmus", "remove-source", "Library"]),
            Ok(CliAction::RemoveSource {
                name: "Library".to_string(),
            })
        );
    }

    #[test]
    fn remove_source_command_requires_name() {
        let error = parse_args(["rmus", "remove-source"]).expect_err("name should be required");
        assert!(error.contains("missing name for remove-source"));

        let error = parse_args(["rmus", "remove-source", "Library", "extra"])
            .expect_err("extra args should fail");
        assert!(error.contains("unexpected argument after source name"));
    }

    #[test]
    fn remove_source_from_config_removes_source_by_name_case_insensitively() {
        let first = test_dir("remove-source-first");
        let second = test_dir("remove-source-second");
        let mut config = crate::config::Config::default();
        add_source_to_config(&mut config, "First", &first).unwrap();
        add_source_to_config(&mut config, "Second", &second).unwrap();

        let summary = remove_source_from_config(&mut config, " second ").unwrap();

        assert_eq!(summary.name, "Second");
        assert_eq!(summary.path, second.canonicalize().unwrap());
        assert_eq!(summary.source_count, 1);
        assert_eq!(config.local.sources.len(), 1);
        assert_eq!(config.local.sources[0].name, "First");
        assert!(summary.message().contains("Removed local source 'Second'"));

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn remove_source_from_config_rejects_missing_source_names() {
        let dir = test_dir("remove-source-missing");
        let mut config = crate::config::Config::default();
        add_source_to_config(&mut config, "Library", &dir).unwrap();

        let error = remove_source_from_config(&mut config, "Other")
            .expect_err("unknown source should fail");

        assert!(error.contains("source not found: Other"));
        assert_eq!(config.local.sources.len(), 1);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scan_local_command_warms_configured_sources() {
        assert_eq!(
            parse_args(["rmus", "scan-local"]),
            Ok(CliAction::ScanLocal { name: None })
        );
        assert_eq!(
            parse_args(["rmus", "scan-local", "Library"]),
            Ok(CliAction::ScanLocal {
                name: Some("Library".to_string())
            })
        );
        let error = parse_args(["rmus", "scan-local", "Library", "extra"])
            .expect_err("extra scan-local args should fail");
        assert!(error.contains("unexpected argument after source name"));

        let dir = test_dir("scan-local");
        fs::write(dir.join("01 - Warmed.flac"), "").unwrap();
        let cache_path = dir.join("local-cache.toml");
        let summary = scan_local_with_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![crate::config::LocalSource {
                        name: "Library".to_string(),
                        path: dir.clone(),
                    }],
                },
                ..crate::config::Config::default()
            },
            None,
            cache_path.clone(),
        )
        .unwrap();

        assert_eq!(summary.source_name, None);
        assert_eq!(summary.source_count, 1);
        assert_eq!(summary.track_count, 1);
        assert!(summary.message().contains("Scanned 1 local track"));
        assert!(cache_path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scan_local_reports_missing_sources() {
        let dir = test_dir("scan-local-missing");
        let summary = scan_local_with_cache_path(
            crate::config::Config::default(),
            None,
            dir.join("cache.toml"),
        )
        .unwrap();

        assert_eq!(summary.source_name, None);
        assert_eq!(summary.source_count, 0);
        assert_eq!(summary.track_count, 0);
        assert_eq!(
            summary.message(),
            "No local sources configured; add folders in Settings first"
        );
        assert!(!dir.join("cache.toml").exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scan_local_command_warms_named_source_and_preserves_existing_cache() {
        let first = test_dir("scan-local-first");
        let second = test_dir("scan-local-second");
        fs::write(first.join("01 - First.flac"), "").unwrap();
        fs::write(second.join("01 - Second.flac"), "").unwrap();
        let cache_path = first.join("local-cache.toml");

        let first_config = crate::config::Config {
            local: crate::config::LocalConfig {
                sources: vec![crate::config::LocalSource {
                    name: "First".to_string(),
                    path: first.clone(),
                }],
            },
            ..crate::config::Config::default()
        };
        scan_local_with_cache_path(first_config, None, cache_path.clone()).unwrap();

        let summary = scan_local_with_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![
                        crate::config::LocalSource {
                            name: "First".to_string(),
                            path: first.clone(),
                        },
                        crate::config::LocalSource {
                            name: "Second".to_string(),
                            path: second.clone(),
                        },
                    ],
                },
                ..crate::config::Config::default()
            },
            Some("second"),
            cache_path.clone(),
        )
        .unwrap();

        assert_eq!(summary.source_name, Some("Second".to_string()));
        assert_eq!(summary.source_count, 1);
        assert_eq!(summary.track_count, 1);
        assert!(summary
            .message()
            .contains("Scanned 1 local track from local source 'Second'"));

        let cache = fs::read_to_string(&cache_path).unwrap();
        assert!(cache.contains("01 - First.flac"));
        assert!(cache.contains("01 - Second.flac"));

        let _ = fs::remove_dir_all(first);
        let _ = fs::remove_dir_all(second);
    }

    #[test]
    fn scan_local_command_rejects_unknown_source_names() {
        let dir = test_dir("scan-local-unknown");
        let error = scan_local_with_cache_path(
            crate::config::Config {
                local: crate::config::LocalConfig {
                    sources: vec![crate::config::LocalSource {
                        name: "Library".to_string(),
                        path: dir.clone(),
                    }],
                },
                ..crate::config::Config::default()
            },
            Some("Other"),
            dir.join("cache.toml"),
        )
        .expect_err("unknown source should fail");

        assert_eq!(error.to_string(), "source not found: Other");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn clear_cache_command_runs_maintenance_action() {
        assert_eq!(
            parse_args(["rmus", "clear-cache"]),
            Ok(CliAction::ClearCache)
        );
    }

    #[test]
    fn import_playlist_command_accepts_path_and_optional_name() {
        assert_eq!(
            parse_args(["rmus", "import-playlist", "/music/mix.m3u"]),
            Ok(CliAction::ImportPlaylist {
                path: PathBuf::from("/music/mix.m3u"),
                name: None,
            })
        );
        assert_eq!(
            parse_args(["rmus", "import-playlist", "/music/mix.m3u", "Road Mix"]),
            Ok(CliAction::ImportPlaylist {
                path: PathBuf::from("/music/mix.m3u"),
                name: Some("Road Mix".to_string()),
            })
        );
    }

    #[test]
    fn export_playlist_command_accepts_name_and_path() {
        assert_eq!(
            parse_args(["rmus", "export-playlist", "Road Mix", "/music/road.m3u8"]),
            Ok(CliAction::ExportPlaylist {
                name: "Road Mix".to_string(),
                path: PathBuf::from("/music/road.m3u8"),
            })
        );
    }

    #[test]
    fn clear_cache_removes_cache_file_and_accepts_missing_file() {
        let dir = test_dir("clear-cache");
        let cache_path = dir.join("local-cache.toml");
        fs::write(&cache_path, "tracks = []").unwrap();

        let summary = clear_cache_at(&cache_path).unwrap();

        assert_eq!(summary.path, cache_path);
        assert!(summary.removed);
        assert!(!summary.path.exists());

        let summary = clear_cache_at(&summary.path).unwrap();
        assert!(!summary.removed);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn import_playlist_command_requires_path() {
        let error = parse_args(["rmus", "import-playlist"]).expect_err("path should be required");

        assert!(error.contains("missing path for import-playlist"));
        assert!(error.contains("Usage:"));
    }

    #[test]
    fn export_playlist_command_requires_name_and_path() {
        let error = parse_args(["rmus", "export-playlist"]).expect_err("name should be required");
        assert!(error.contains("missing playlist name for export-playlist"));

        let error = parse_args(["rmus", "export-playlist", "Road Mix"])
            .expect_err("path should be required");
        assert!(error.contains("missing path for export-playlist"));
    }

    #[test]
    fn unknown_args_are_errors() {
        let error = parse_args(["rmus", "--wat"]).expect_err("unknown flag should fail");

        assert!(error.contains("unknown argument '--wat'"));
        assert!(error.contains("Usage:"));
    }

    #[test]
    fn doctor_reports_missing_mpv_as_error() {
        let report = doctor_report_with_options(doctor_options(None));
        let text = report.to_text();

        assert_eq!(report.exit_code(), 1);
        assert!(text.contains("[error] mpv: not found in PATH"));
    }

    #[test]
    fn doctor_accepts_fake_mpv_and_missing_config_as_warning() {
        let path_env = env::join_paths([fake_mpv_dir()]).unwrap();
        let report = doctor_report_with_options(doctor_options(Some(path_env)));
        let text = report.to_text();

        assert_eq!(report.exit_code(), 0);
        assert!(text.contains("[ok] mpv:"));
        assert!(text.contains("[warn] config:"));
        assert!(text.contains("[warn] queue:"));
        assert!(text.contains("[warn] local cache:"));
        assert!(text.contains("[warn] local sources: config missing"));
    }

    #[test]
    fn doctor_warns_about_missing_local_source_dirs() {
        let dir = test_dir("missing-local-source");
        let config_path = dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
                [[local.sources]]
                name = "Gone"
                path = "/definitely/missing/rmus/source"

                [audio]
                default_volume = 50
            "#,
        )
        .unwrap();

        let path_env = env::join_paths([fake_mpv_dir()]).unwrap();
        let report = doctor_report_with_options(DoctorOptions {
            path_env: Some(path_env),
            config_path,
            playlists_dir: dir.join("playlists"),
            history_path: dir.join("history.toml"),
            queue_path: dir.join("queue.toml"),
            local_cache_path: dir.join("local-cache.toml"),
        });
        let text = report.to_text();

        assert_eq!(report.exit_code(), 0);
        assert!(text.contains("[warn] local sources: 1 missing"));
    }
}
