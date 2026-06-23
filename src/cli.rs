use std::{
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
    sources::local::{LocalFiles, LocalLibraryStats},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Help,
    Version,
    Doctor,
    Paths,
    ListSources,
    LocalStats,
    ScanLocal,
    AddSource { name: String, path: PathBuf },
    RemoveSource { name: String },
    ImportPlaylist { path: PathBuf, name: Option<String> },
    ExportPlaylist { name: String, path: PathBuf },
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
        "local-stats" => no_more_args(args, CliAction::LocalStats, &first),
        "scan-local" => no_more_args(args, CliAction::ScanLocal, &first),
        "add-source" => parse_add_source_args(args),
        "remove-source" => parse_remove_source_args(args),
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
    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after source path\n\n{}",
            help_text()
        ));
    }

    Ok(CliAction::AddSource {
        name,
        path: PathBuf::from(path),
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
        "  local-stats     Count configured local sources, albums, and tracks\n",
        "  scan-local      Scan configured local sources into the metadata cache\n",
        "  add-source <NAME> <PATH>\n",
        "                  Add a local music folder to config\n",
        "  remove-source <NAME>\n",
        "                  Remove a local music folder from config\n",
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

fn expand_home_path(path: &Path) -> PathBuf {
    let path_text = path.to_string_lossy();
    if path_text == "~" {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = path_text.strip_prefix("~/") {
        return env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(rest))
            .unwrap_or_else(|| path.to_path_buf());
    }
    path.to_path_buf()
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
    pub source_count: usize,
    pub track_count: usize,
    pub cache_path: PathBuf,
}

impl LocalScanSummary {
    pub fn message(&self) -> String {
        if self.source_count == 0 {
            return "No local sources configured; add folders in Settings first".to_string();
        }

        format!(
            "Scanned {} local {} from {} local {}; cache: {}",
            self.track_count,
            plural(self.track_count, "track", "tracks"),
            self.source_count,
            plural(self.source_count, "source", "sources"),
            self.cache_path.to_string_lossy()
        )
    }
}

pub fn scan_local() -> Result<LocalScanSummary, std::io::Error> {
    scan_local_with_config(Config::load())
}

fn scan_local_with_config(config: Config) -> Result<LocalScanSummary, std::io::Error> {
    scan_local_with_cache_path(config, LocalTrackCache::default_path())
}

fn scan_local_with_cache_path(
    config: Config,
    cache_path: PathBuf,
) -> Result<LocalScanSummary, std::io::Error> {
    let sources = config.get_local_sources();
    let track_count = if sources.is_empty() {
        0
    } else {
        LocalFiles::scan_sources_with_cache_path(&sources, cache_path.clone())?
    };

    Ok(LocalScanSummary {
        source_count: sources.len(),
        track_count,
        cache_path,
    })
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
        add_source_to_config, clear_cache_at, doctor_report_with_options, list_sources_from_config,
        local_stats_with_cache_path, parse_args, paths_text_with_options,
        remove_source_from_config, scan_local_with_cache_path, CliAction, DoctorOptions,
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
    fn add_source_command_accepts_name_and_path() {
        assert_eq!(
            parse_args(["rmus", "add-source", "Library", "/music"]),
            Ok(CliAction::AddSource {
                name: "Library".to_string(),
                path: PathBuf::from("/music"),
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
        assert_eq!(parse_args(["rmus", "scan-local"]), Ok(CliAction::ScanLocal));

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
            cache_path.clone(),
        )
        .unwrap();

        assert_eq!(summary.source_count, 1);
        assert_eq!(summary.track_count, 1);
        assert!(summary.message().contains("Scanned 1 local track"));
        assert!(cache_path.exists());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scan_local_reports_missing_sources() {
        let dir = test_dir("scan-local-missing");
        let summary =
            scan_local_with_cache_path(crate::config::Config::default(), dir.join("cache.toml"))
                .unwrap();

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
