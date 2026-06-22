use base64::Engine;
use md5::compute as md5_compute;
use regex::Regex;
use std::collections::HashMap;

use crate::config::MaxStreamQuality;
use crate::sources::qobuz::types::{
    AlbumSearchResponse, ArtistAlbumsResponse, ArtistSearchResponse, LoginResponse, SearchResponse,
    StreamUrlResponse,
};
use crate::sources::streaming::{
    AuthStatus, ResolvedStream, ResolvedStreamSource, StreamAlbum, StreamArtist, StreamTrack,
    StreamingService,
};

const BASE_URL: &str = "https://www.qobuz.com/api.json/0.2";

// --- Async API client ---

struct QobuzClient {
    app_id: String,
    app_secret: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl QobuzClient {
    fn new(app_id: String, app_secret: String) -> Self {
        Self {
            app_id,
            app_secret,
            token: None,
            client: reqwest::Client::new(),
        }
    }

    async fn login(&mut self, email: &str, password: &str) -> Result<bool, reqwest::Error> {
        let pwd_hash = format!("{:x}", md5_compute(password.as_bytes()));

        let resp: LoginResponse = self
            .client
            .get(format!("{BASE_URL}/user/login"))
            .query(&[
                ("email", email),
                ("password", &pwd_hash),
                ("app_id", &self.app_id),
            ])
            .send()
            .await?
            .json()
            .await?;

        if let Some(token) = resp.user_auth_token {
            self.token = Some(token);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<StreamTrack>, reqwest::Error> {
        let resp: SearchResponse = self
            .client
            .get(format!("{BASE_URL}/track/search"))
            .query(&[
                ("query", query),
                ("limit", &limit.to_string()),
                ("app_id", &self.app_id),
            ])
            .send()
            .await?
            .json()
            .await?;

        let tracks = resp
            .tracks
            .and_then(|t| t.items)
            .unwrap_or_default()
            .into_iter()
            .map(|t| StreamTrack {
                id: t.id.to_string(),
                title: t.title.unwrap_or_else(|| "Unknown".to_string()),
                artist: t
                    .performer
                    .and_then(|p| p.name)
                    .unwrap_or_else(|| "Unknown".to_string()),
                album: t
                    .album
                    .and_then(|a| a.title)
                    .unwrap_or_else(|| "Unknown".to_string()),
            })
            .collect();

        Ok(tracks)
    }

    async fn search_albums(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamAlbum>, reqwest::Error> {
        let resp: AlbumSearchResponse = self
            .client
            .get(format!("{BASE_URL}/album/search"))
            .query(&[
                ("query", query),
                ("limit", &limit.to_string()),
                ("app_id", &self.app_id),
            ])
            .send()
            .await?
            .json()
            .await?;

        let albums = resp
            .albums
            .and_then(|a| a.items)
            .unwrap_or_default()
            .into_iter()
            .map(|a| StreamAlbum {
                id: a.id.to_string(),
                title: a.title.unwrap_or_else(|| "Unknown".to_string()),
                artist: a
                    .artist
                    .and_then(|p| p.name)
                    .unwrap_or_else(|| "Unknown".to_string()),
                track_count: a.tracks_count,
            })
            .collect();

        Ok(albums)
    }

    async fn search_artists(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamArtist>, reqwest::Error> {
        let resp: ArtistSearchResponse = self
            .client
            .get(format!("{BASE_URL}/artist/search"))
            .query(&[
                ("query", query),
                ("limit", &limit.to_string()),
                ("app_id", &self.app_id),
            ])
            .send()
            .await?
            .json()
            .await?;

        let artists = resp
            .artists
            .and_then(|a| a.items)
            .unwrap_or_default()
            .into_iter()
            .map(|a| StreamArtist {
                id: a.id.to_string(),
                name: a.name.unwrap_or_else(|| "Unknown".to_string()),
                album_count: a.albums_count,
            })
            .collect();

        Ok(artists)
    }

    async fn get_artist_albums(&self, artist_id: &str) -> Result<Vec<StreamAlbum>, reqwest::Error> {
        let resp: ArtistAlbumsResponse = self
            .client
            .get(format!("{BASE_URL}/artist/get"))
            .query(&[
                ("artist_id", artist_id),
                ("app_id", &self.app_id),
                ("extra", "albums"),
                ("limit", "100"),
            ])
            .send()
            .await?
            .json()
            .await?;

        let albums = resp
            .albums
            .and_then(|a| a.items)
            .unwrap_or_default()
            .into_iter()
            .map(|a| StreamAlbum {
                id: a.id.to_string(),
                title: a.title.unwrap_or_else(|| "Unknown".to_string()),
                artist: a
                    .artist
                    .and_then(|p| p.name)
                    .unwrap_or_else(|| "Unknown".to_string()),
                track_count: a.tracks_count,
            })
            .collect();

        Ok(albums)
    }

    async fn get_album_tracks(&self, album_id: &str) -> Result<Vec<StreamTrack>, reqwest::Error> {
        let resp: crate::sources::qobuz::types::AlbumResponse = self
            .client
            .get(format!("{BASE_URL}/album/get"))
            .query(&[("album_id", album_id), ("app_id", &self.app_id)])
            .send()
            .await?
            .json()
            .await?;

        let tracks = resp
            .tracks
            .and_then(|t| t.items)
            .unwrap_or_default()
            .into_iter()
            .map(|t| StreamTrack {
                id: t.id.to_string(),
                title: t.title.unwrap_or_else(|| "Unknown".to_string()),
                artist: t
                    .performer
                    .and_then(|p| p.name)
                    .unwrap_or_else(|| "Unknown".to_string()),
                album: String::new(),
            })
            .collect();

        Ok(tracks)
    }

    async fn get_stream_url(
        &self,
        track_id: &str,
        format_id: u32,
    ) -> Result<Option<String>, reqwest::Error> {
        let ts = chrono::Utc::now().timestamp();
        let sig_input = format!(
            "trackgetFileUrlformat_id{format_id}intentstreamtrack_id{track_id}{ts}{}",
            self.app_secret
        );
        let sig = format!("{:x}", md5_compute(sig_input.as_bytes()));
        let token = self.token.as_deref().unwrap_or("");

        let resp = self
            .client
            .get(format!("{BASE_URL}/track/getFileUrl"))
            .query(&[
                ("track_id", track_id),
                ("format_id", &format_id.to_string()),
                ("intent", "stream"),
                ("request_ts", &ts.to_string()),
                ("request_sig", &sig),
                ("app_id", &self.app_id),
                ("user_auth_token", token),
            ])
            .send()
            .await?;

        if resp.status().is_success() {
            let data: StreamUrlResponse = resp.json().await?;
            Ok(data.url)
        } else {
            Ok(None)
        }
    }
}

// --- Credential fetching ---

async fn get_app_id_and_secret() -> Result<(String, String), Box<dyn std::error::Error>> {
    let seed_timezone_regex = Regex::new(
        r#"[a-z]\.initialSeed\("(?P<seed>[\w=]+)",window\.utimezone\.(?P<timezone>[a-z]+)\)"#,
    )?;
    let app_id_regex =
        Regex::new(r#"production:\{api:\{appId:"(?P<app_id>\d{9})",appSecret:"(\w{32})"#)?;

    let client = reqwest::Client::new();

    let login_page = client
        .get("https://play.qobuz.com/login")
        .send()
        .await?
        .text()
        .await?;

    let bundle_regex =
        Regex::new(r#"<script src="(/resources/\d+\.\d+\.\d+-[a-z]\d{3}/bundle\.js)"></script>"#)?;

    let bundle_path = bundle_regex
        .captures(&login_page)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or("Could not find bundle URL")?;

    let bundle = client
        .get(format!("https://play.qobuz.com{}", bundle_path))
        .send()
        .await?
        .text()
        .await?;

    let app_id = app_id_regex
        .captures(&bundle)
        .and_then(|c| c.name("app_id"))
        .map(|m| m.as_str().to_string())
        .ok_or("Could not find app ID")?;

    let mut secrets: HashMap<String, Vec<String>> = HashMap::new();
    for cap in seed_timezone_regex.captures_iter(&bundle) {
        let tz = cap.name("timezone").unwrap().as_str().to_string();
        let seed = cap.name("seed").unwrap().as_str().to_string();
        secrets.insert(tz, vec![seed]);
    }

    let timezones: String = secrets
        .keys()
        .map(|tz| {
            let mut chars = tz.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("|");

    let info_extras_regex = Regex::new(&format!(
        r#"name:"\w+/(?P<timezone>{timezones})",info:"(?P<info>[\w=]+)",extras:"(?P<extras>[\w=]+)""#
    ))?;

    for cap in info_extras_regex.captures_iter(&bundle) {
        let tz = cap.name("timezone").unwrap().as_str().to_lowercase();
        if let Some(arr) = secrets.get_mut(&tz) {
            arr.push(cap.name("info").unwrap().as_str().to_string());
            arr.push(cap.name("extras").unwrap().as_str().to_string());
        }
    }

    let mut decoded_secrets = Vec::new();
    for arr in secrets.values() {
        let combined: String = arr.join("");
        if combined.len() > 44 {
            let trimmed = &combined[..combined.len() - 44];
            if let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(trimmed) {
                if let Ok(decoded) = String::from_utf8(decoded_bytes) {
                    if !decoded.is_empty() {
                        decoded_secrets.push(decoded);
                    }
                }
            }
        }
    }

    for secret in &decoded_secrets {
        let now = chrono::Utc::now();
        let ts = now.timestamp() as f64 + now.timestamp_subsec_micros() as f64 / 1_000_000.0;
        let sig_input = format!("trackgetFileUrlformat_id27intentstreamtrack_id1{ts}{secret}");
        let sig = format!("{:x}", md5_compute(sig_input.as_bytes()));

        let url = format!(
            "{BASE_URL}/track/getFileUrl?format_id=27&intent=stream&track_id=1&request_ts={ts}&request_sig={sig}"
        );

        let resp = client.get(&url).header("X-App-Id", &app_id).send().await?;

        if resp.status().as_u16() != 400 {
            return Ok((app_id, secret.clone()));
        }
    }

    Err("No valid secret found".into())
}

fn make_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
}

// --- Public streaming source ---

pub struct QobuzSource {
    client: Option<QobuzClient>,
    app_id: Option<String>,
    app_secret: Option<String>,
    email: String,
    password: String,
    max_stream_quality: MaxStreamQuality,
}

impl QobuzSource {
    pub fn new() -> Self {
        Self {
            client: None,
            app_id: None,
            app_secret: None,
            email: String::new(),
            password: String::new(),
            max_stream_quality: MaxStreamQuality::default(),
        }
    }

    /// Create with cached app credentials and login credentials from config.
    pub fn with_credentials(
        app_id: String,
        app_secret: String,
        email: String,
        password: String,
        max_stream_quality: MaxStreamQuality,
    ) -> Self {
        Self {
            client: None,
            app_id: if app_id.is_empty() {
                None
            } else {
                Some(app_id)
            },
            app_secret: if app_secret.is_empty() {
                None
            } else {
                Some(app_secret)
            },
            email,
            password,
            max_stream_quality,
        }
    }
}

impl Default for QobuzSource {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingService for QobuzSource {
    fn name(&self) -> &str {
        "Qobuz"
    }

    fn is_authenticated(&self) -> bool {
        self.client
            .as_ref()
            .map(|c| c.token.is_some())
            .unwrap_or(false)
    }

    fn authenticate(&mut self) -> Result<AuthStatus, Box<dyn std::error::Error>> {
        if self.email.is_empty() || self.password.is_empty() {
            return Err("No Qobuz credentials configured. Set them in Settings > Account.".into());
        }

        let rt = make_runtime();

        if self.app_id.is_none() || self.app_secret.is_none() {
            let (id, secret) = rt.block_on(get_app_id_and_secret())?;
            self.app_id = Some(id);
            self.app_secret = Some(secret);
        }

        let mut client = QobuzClient::new(
            self.app_id.clone().unwrap(),
            self.app_secret.clone().unwrap(),
        );

        let success = rt.block_on(client.login(&self.email, &self.password))?;
        if success {
            self.client = Some(client);
            Ok(AuthStatus::Authenticated)
        } else {
            Err("Login failed - check your Qobuz credentials".into())
        }
    }

    fn search(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        let client = self.client.as_ref().ok_or("Not authenticated with Qobuz")?;
        let rt = make_runtime();
        let tracks = rt.block_on(client.search(query, limit))?;
        Ok(tracks)
    }

    fn search_albums(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamAlbum>, Box<dyn std::error::Error>> {
        let client = self.client.as_ref().ok_or("Not authenticated with Qobuz")?;
        let rt = make_runtime();
        let albums = rt.block_on(client.search_albums(query, limit))?;
        Ok(albums)
    }

    fn search_artists(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamArtist>, Box<dyn std::error::Error>> {
        let client = self.client.as_ref().ok_or("Not authenticated with Qobuz")?;
        let rt = make_runtime();
        let artists = rt.block_on(client.search_artists(query, limit))?;
        Ok(artists)
    }

    fn get_artist_albums(
        &mut self,
        artist_id: &str,
    ) -> Result<Vec<StreamAlbum>, Box<dyn std::error::Error>> {
        let client = self.client.as_ref().ok_or("Not authenticated with Qobuz")?;
        let rt = make_runtime();
        let albums = rt.block_on(client.get_artist_albums(artist_id))?;
        Ok(albums)
    }

    fn get_album_tracks(
        &mut self,
        album_id: &str,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        let client = self.client.as_ref().ok_or("Not authenticated with Qobuz")?;
        let rt = make_runtime();
        let tracks = rt.block_on(client.get_album_tracks(album_id))?;
        Ok(tracks)
    }

    fn get_stream_url(
        &mut self,
        track_id: &str,
    ) -> Result<Option<ResolvedStream>, Box<dyn std::error::Error>> {
        let client = self.client.as_ref().ok_or("Not authenticated with Qobuz")?;
        let rt = make_runtime();
        let url = rt
            .block_on(client.get_stream_url(track_id, self.max_stream_quality.qobuz_format_id()))?;
        Ok(url.map(|url| ResolvedStream {
            source: ResolvedStreamSource::Url(url),
            quality_label: Some(self.max_stream_quality.qobuz_quality_label().to_string()),
        }))
    }

    fn app_credentials(&self) -> Option<(String, String)> {
        match (&self.app_id, &self.app_secret) {
            (Some(id), Some(secret)) => Some((id.clone(), secret.clone())),
            _ => None,
        }
    }
}

impl std::fmt::Debug for QobuzSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QobuzSource")
            .field("authenticated", &self.is_authenticated())
            .field("has_app_id", &self.app_id.is_some())
            .finish()
    }
}
