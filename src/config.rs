use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub local_paths: Vec<PathBuf>,
    pub audio: AudioConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AudioConfig {
    pub default_volume: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            local_paths: Vec::new(),
            audio: AudioConfig { default_volume: 50 },
        }
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
}

fn get_config_path() -> PathBuf {
    ProjectDirs::from("com", "your_name", "rmus")
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}
