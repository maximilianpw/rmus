use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct StreamTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
}

impl StreamTrack {
    pub fn display_title(&self) -> String {
        format!("{} - {}", self.artist, self.title)
    }
}

/// Trait for online music streaming services (Qobuz, Tidal, etc.)
///
/// All methods are blocking - implementations handle their own async
/// runtimes internally. This keeps the synchronous TUI loop simple.
pub trait StreamingService: Debug {
    fn name(&self) -> &str;
    fn is_authenticated(&self) -> bool;

    /// Authenticate with the service using stored credentials.
    /// Implementations handle any service-specific setup (e.g. fetching API keys).
    fn authenticate(
        &mut self,
        email: &str,
        password: &str,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Search for tracks by query string.
    fn search(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>>;

    /// Get a playable stream URL for a track by its ID.
    fn get_stream_url(
        &self,
        track_id: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error>>;

    /// Returns service-specific app credentials (id, secret) for caching in config.
    /// Services that don't need credential caching can leave the default (None).
    fn app_credentials(&self) -> Option<(String, String)> {
        None
    }
}
