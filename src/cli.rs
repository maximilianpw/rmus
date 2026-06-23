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
    playlist::PlaylistStore,
    queue::QueueStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    Run,
    Help,
    Version,
    Doctor,
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

    if args.next().is_some() {
        return Err(format!(
            "unexpected argument after '{first}'\n\n{}",
            help_text()
        ));
    }

    match first.as_str() {
        "-h" | "--help" => Ok(CliAction::Help),
        "-V" | "--version" => Ok(CliAction::Version),
        "doctor" => Ok(CliAction::Doctor),
        _ => Err(format!("unknown argument '{first}'\n\n{}", help_text())),
    }
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
        "\n",
        "Options:\n",
        "  -h, --help       Print help\n",
        "  -V, --version    Print version\n"
    )
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
    use super::{doctor_report_with_options, parse_args, CliAction, DoctorOptions};
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
