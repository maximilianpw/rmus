use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    pub local: LocalConfig,
    #[serde(default)]
    pub qobuz: Option<QobuzConfig>,
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
            qobuz: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_without_qobuz() {
        let toml = r#"
            [[local.sources]]
            name = "Test Album"
            path = "/music/test"

            [audio]
            default_volume = 75
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.qobuz.is_none());
        assert_eq!(config.audio.default_volume, 75);
        assert_eq!(config.local.sources.len(), 1);
        assert_eq!(config.local.sources[0].name, "Test Album");
    }

    #[test]
    fn test_parse_config_with_qobuz() {
        let toml = r#"
            [[local.sources]]
            name = "Test"
            path = "/music"

            [qobuz]
            email = "test@example.com"
            password = "secret"
            app_id = "12345"

            [audio]
            default_volume = 50
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.qobuz.is_some());
        let qobuz = config.qobuz.unwrap();
        assert_eq!(qobuz.email, "test@example.com");
        assert_eq!(qobuz.app_id, "12345");
    }

    #[test]
    fn test_parse_multiple_sources() {
        let toml = r#"
            [[local.sources]]
            name = "Album 1"
            path = "/music/album1"

            [[local.sources]]
            name = "Album 2"
            path = "/music/album2"

            [audio]
            default_volume = 50
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.local.sources.len(), 2);
        assert_eq!(config.local.sources[0].name, "Album 1");
        assert_eq!(config.local.sources[1].name, "Album 2");
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.local.sources.is_empty());
        assert!(config.qobuz.is_none());
        assert_eq!(config.audio.default_volume, 50);
    }

    #[test]
    fn test_get_local_sources() {
        let config = Config {
            local: LocalConfig {
                sources: vec![LocalSource {
                    name: "Test".to_string(),
                    path: PathBuf::from("/test"),
                }],
            },
            qobuz: None,
            audio: AudioConfig { default_volume: 50 },
        };

        let sources = config.get_local_sources();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "Test");
    }
}
