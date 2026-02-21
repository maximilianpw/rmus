use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Display};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub local: LocalConfig,
    #[serde(default)]
    pub qobuz: Option<QobuzConfig>,
    #[serde(default)]
    pub tidal: Option<TidalConfig>,
    pub audio: AudioConfig,
    #[serde(default = "default_streaming_service")]
    pub streaming_service: String,
}

fn default_streaming_service() -> String {
    "qobuz".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalSource {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LocalConfig {
    pub sources: Vec<LocalSource>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct QobuzConfig {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TidalConfig {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub country_code: String,
    #[serde(default)]
    pub token_expiry: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
            tidal: None,
            audio: AudioConfig { default_volume: 50 },
            streaming_service: default_streaming_service(),
        }
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("local", &self.local)
            .field("qobuz", &self.qobuz)
            .field("tidal", &self.tidal)
            .field("audio", &self.audio)
            .field("streaming_service", &self.streaming_service)
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

    pub fn add_local_source(&mut self, name: String, path: PathBuf) {
        self.local.sources.push(LocalSource { name, path });
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
        assert!(config.tidal.is_none());
        assert_eq!(config.streaming_service, "qobuz");
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
    fn test_parse_config_with_tidal() {
        let toml = r#"
            streaming_service = "tidal"

            [[local.sources]]
            name = "Test"
            path = "/music"

            [tidal]
            access_token = "abc123"
            refresh_token = "def456"
            country_code = "US"
            token_expiry = 1700000000

            [audio]
            default_volume = 50
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.tidal.is_some());
        let tidal = config.tidal.unwrap();
        assert_eq!(tidal.access_token, "abc123");
        assert_eq!(tidal.refresh_token, "def456");
        assert_eq!(tidal.country_code, "US");
        assert_eq!(tidal.token_expiry, 1700000000);
        assert_eq!(config.streaming_service, "tidal");
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
        assert!(config.tidal.is_none());
        assert_eq!(config.streaming_service, "qobuz");
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
            tidal: None,
            audio: AudioConfig { default_volume: 50 },
            streaming_service: "qobuz".to_string(),
        };

        let sources = config.get_local_sources();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "Test");
    }

    #[test]
    fn test_tidal_credentials_round_trip() {
        let config = Config {
            local: LocalConfig {
                sources: Vec::new(),
            },
            qobuz: None,
            tidal: Some(TidalConfig {
                access_token: "abc123".to_string(),
                refresh_token: "def456".to_string(),
                country_code: "US".to_string(),
                token_expiry: 1700000000,
            }),
            audio: AudioConfig { default_volume: 50 },
            streaming_service: "tidal".to_string(),
        };

        // Verify TOML serialization includes [tidal] section
        let toml_string = toml::to_string_pretty(&config).unwrap();
        assert!(
            toml_string.contains("[tidal]"),
            "TOML should contain [tidal] section, got:\n{}",
            toml_string
        );
        assert!(toml_string.contains("abc123"));
        assert!(toml_string.contains("def456"));

        // Verify round-trip through TOML
        let loaded: Config = toml::from_str(&toml_string).unwrap();
        assert_eq!(loaded.streaming_service, "tidal");
        let tidal = loaded.tidal.unwrap();
        assert_eq!(tidal.access_token, "abc123");
        assert_eq!(tidal.refresh_token, "def456");
        assert_eq!(tidal.country_code, "US");
        assert_eq!(tidal.token_expiry, 1700000000);

        // Verify JSON round-trip (persist_data path)
        let json = serde_json::to_string(&config.tidal.as_ref().unwrap()).unwrap();
        let from_json: TidalConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(from_json.access_token, "abc123");
    }
}
