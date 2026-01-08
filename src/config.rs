use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub local: LocalConfig,
    pub qobuz: QobuzConfig,
    pub audio: AudioConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalSource {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalConfig {
    pub sources: Vec<LocalSource>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QobuzConfig {
    pub email: String,
    pub password: String,
    pub app_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioConfig {
    pub default_volume: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            local: LocalConfig {
                sources: Vec::new(),
            },
            qobuz: QobuzConfig {
                email: String::new(),
                password: String::new(),
                app_id: String::new(),
            },
            audio: AudioConfig { default_volume: 50 },
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("local", &self.local)
            .field("qobuz", &self.qobuz)
            .field("audio", &self.audio)
            .finish()
    }
}

impl Config {
    pub fn load() -> Self {
        let config_path = get_config_path();

        if let Ok(content) = fs::read_to_string(&config_path) {
            toml::from_str(&content).unwrap_or_default()
        } else {
            let default = Config::default();
            default.save().ok(); // Try to create the file if it doesn't exist
            default
        }
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let config_path = get_config_path();
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let toml_string = toml::to_string_pretty(self).unwrap();
        fs::write(config_path, toml_string)
    }

    pub fn get_local_sources(&self) -> Vec<LocalSource> {
        self.local
            .sources
            .iter()
            .map(|s| LocalSource {
                name: s.name.clone(),
                path: s.path.clone(),
            })
            .collect()
    }
}

fn get_config_path() -> PathBuf {
    ProjectDirs::from("com", "maximilianpw", "rmus")
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}
