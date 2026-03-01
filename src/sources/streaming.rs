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

    /// Get a playable stream URL for a track by its ID.
    fn get_stream_url(
        &mut self,
        track_id: &str,
    ) -> Result<Option<String>, Box<dyn std::error::Error>>;

    /// Returns service-specific app credentials (id, secret) for caching in config.
    /// Services that don't need credential caching can leave the default (None).
    fn app_credentials(&self) -> Option<(String, String)> {
        None
    }
}
