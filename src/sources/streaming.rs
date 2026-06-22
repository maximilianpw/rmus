use std::fmt::Debug;

use crate::utils::track_count_label;

#[derive(Debug, Clone)]
pub struct StreamTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
}

impl StreamTrack {
    pub fn display_title(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamAlbum {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub track_count: Option<u32>,
}

impl StreamAlbum {
    pub fn display_title(&self) -> String {
        match self.track_count {
            Some(n) => format!(
                "{} - {} ({})",
                self.artist,
                self.title,
                track_count_label(n as usize)
            ),
            None => format!("{} - {}", self.artist, self.title),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedStreamSource {
    Url(String),
    Manifest {
        contents: String,
        file_extension: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStream {
    pub source: ResolvedStreamSource,
    pub quality_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingServiceId {
    Qobuz,
    Tidal,
}

impl StreamingServiceId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Qobuz => "Qobuz",
            Self::Tidal => "Tidal",
        }
    }

    pub fn from_tab_name(name: &str) -> Option<Self> {
        match name {
            "Qobuz" => Some(Self::Qobuz),
            "Tidal" => Some(Self::Tidal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamArtist {
    pub id: String,
    pub name: String,
    pub album_count: Option<u32>,
}

impl StreamArtist {
    pub fn display_title(&self) -> String {
        match self.album_count {
            Some(n) => format!("{} ({} albums)", self.name, n),
            None => self.name.clone(),
        }
    }
}

/// Result of an authentication attempt.
#[derive(Debug)]
pub enum AuthStatus {
    /// Fully authenticated and ready to use.
    Authenticated,
    /// Waiting for user action (e.g. device auth code entry).
    /// The string contains a message to display to the user.
    PendingUserAction(String),
}

/// Trait for online music streaming services (Qobuz, Tidal, etc.)
///
/// All methods are blocking - implementations handle their own async
/// runtimes internally. This keeps the synchronous TUI loop simple.
pub trait StreamingService: Debug + Send {
    fn name(&self) -> &str;
    fn is_authenticated(&self) -> bool;

    /// Authenticate with the service using internally stored credentials.
    fn authenticate(&mut self) -> Result<AuthStatus, Box<dyn std::error::Error>>;

    /// Poll for pending auth completion (e.g. device code flow).
    /// Returns Ok(true) when auth is complete, Ok(false) when still pending.
    /// Services that don't need polling can use the default implementation.
    fn poll_auth(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(true)
    }

    /// Returns serialized data for token persistence.
    /// Services that don't need persistence can use the default implementation.
    fn persist_data(&self) -> Option<String> {
        None
    }

    /// Search for tracks by query string.
    fn search(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>>;

    /// Search for albums by query string.
    fn search_albums(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamAlbum>, Box<dyn std::error::Error>>;

    /// Get all tracks for an album by its ID.
    fn get_album_tracks(
        &mut self,
        album_id: &str,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>>;

    /// Search for artists by query string.
    fn search_artists(
        &mut self,
        _query: &str,
        _limit: u32,
    ) -> Result<Vec<StreamArtist>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    /// Get all albums for an artist by their ID.
    fn get_artist_albums(
        &mut self,
        _artist_id: &str,
    ) -> Result<Vec<StreamAlbum>, Box<dyn std::error::Error>> {
        Ok(vec![])
    }

    /// Get a playable stream URL for a track by its ID.
    fn get_stream_url(
        &mut self,
        track_id: &str,
    ) -> Result<Option<ResolvedStream>, Box<dyn std::error::Error>>;

    /// Returns service-specific app credentials (id, secret) for caching in config.
    /// Services that don't need credential caching can leave the default (None).
    fn app_credentials(&self) -> Option<(String, String)> {
        None
    }
}
