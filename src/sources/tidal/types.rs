use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct DeviceAuthResponse {
    #[serde(rename = "deviceCode")]
    pub(crate) device_code: String,
    #[serde(rename = "userCode")]
    pub(crate) user_code: String,
    #[serde(rename = "verificationUri")]
    pub(crate) verification_uri: String,
    #[serde(rename = "expiresIn")]
    pub(crate) _expires_in: u64,
    pub(crate) interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in: u64,
    pub(crate) user: Option<TokenUser>,
}

#[derive(Debug, Deserialize)]
pub struct TokenUser {
    #[serde(rename = "countryCode")]
    pub(crate) country_code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TokenErrorResponse {
    #[serde(rename = "sub_status")]
    pub(crate) sub_status: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct TidalSearchResponse {
    pub(crate) tracks: Option<TidalTracksContainer>,
}

#[derive(Debug, Deserialize)]
pub struct TidalTracksContainer {
    pub(crate) items: Option<Vec<TidalTrackResponse>>,
}

#[derive(Debug, Deserialize)]
pub struct TidalTrackResponse {
    pub(crate) id: u64,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<TidalArtist>,
    pub(crate) artists: Option<Vec<TidalArtist>>,
    pub(crate) album: Option<TidalAlbum>,
}

#[derive(Debug, Deserialize)]
pub struct TidalArtist {
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TidalAlbum {
    pub(crate) title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlaybackInfoResponse {
    pub(crate) manifest: Option<String>,
    #[serde(rename = "manifestMimeType")]
    pub(crate) manifest_mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestJson {
    pub(crate) urls: Option<Vec<String>>,
}
