use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct LoginResponse {
    pub(crate) user_auth_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SearchResponse {
    pub(crate) tracks: Option<TracksContainer>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TracksContainer {
    pub(crate) items: Option<Vec<TrackResponse>>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct TrackResponse {
    pub(crate) id: u64,
    pub(crate) title: Option<String>,
    pub(crate) performer: Option<Performer>,
    pub(crate) album: Option<AlbumInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Performer {
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct AlbumInfo {
    pub(crate) title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AlbumSearchResponse {
    pub(crate) albums: Option<AlbumsContainer>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AlbumsContainer {
    pub(crate) items: Option<Vec<AlbumResponse>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AlbumResponse {
    pub(crate) id: String,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<Performer>,
    pub(crate) tracks_count: Option<u32>,
    pub(crate) tracks: Option<TracksContainer>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StreamUrlResponse {
    pub(crate) url: Option<String>,
}
