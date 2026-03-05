use base64::Engine;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::{MaxStreamQuality, TidalConfig};
use crate::sources::streaming::{
    AuthStatus, ResolvedStream, ResolvedStreamSource, StreamTrack, StreamingService,
};
use crate::sources::tidal::types::{
    DeviceAuthResponse, ManifestJson, PlaybackInfoResponse, TidalSearchResponse,
    TokenErrorResponse, TokenResponse,
};

const AUTH_URL: &str = "https://auth.tidal.com/v1/oauth2";
const API_URL: &str = "https://api.tidal.com/v1";
const CLIENT_ID: &str = "fX2JxdmntZWK0ixT";
const CLIENT_SECRET: &str = "1Nn9AfDAjxrgJFJbKNWLeAyKGVGmINuXPPLHVXAvxAg=";
const POLL_INTERVAL: Duration = Duration::from_secs(2);

// --- Async API client ---

struct TidalClient {
    access_token: String,
    country_code: String,
    client: reqwest::Client,
}

impl TidalClient {
    fn new(access_token: String, country_code: String) -> Self {
        Self {
            access_token,
            country_code,
            client: reqwest::Client::new(),
        }
    }

    async fn search(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        let country = if self.country_code.is_empty() {
            "US"
        } else {
            &self.country_code
        };

        let resp = self
            .client
            .get(format!("{API_URL}/search"))
            .bearer_auth(&self.access_token)
            .query(&[
                ("query", query),
                ("limit", &limit.to_string()),
                ("countryCode", country),
                ("types", "TRACKS"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Tidal search failed ({}): {}", status, body).into());
        }

        let search_resp: TidalSearchResponse = resp.json().await?;

        let tracks = search_resp
            .tracks
            .and_then(|t| t.items)
            .unwrap_or_default()
            .into_iter()
            .map(|t| {
                let artist_name = t
                    .artist
                    .and_then(|a| a.name)
                    .or_else(|| t.artists.and_then(|mut v| v.pop()).and_then(|a| a.name))
                    .unwrap_or_else(|| "Unknown".to_string());
                StreamTrack {
                    id: t.id.to_string(),
                    title: t.title.unwrap_or_else(|| "Unknown".to_string()),
                    artist: artist_name,
                    album: t
                        .album
                        .and_then(|a| a.title)
                        .unwrap_or_else(|| "Unknown".to_string()),
                }
            })
            .collect();

        Ok(tracks)
    }

    fn extract_stream_source_from_manifest(
        manifest_b64: &str,
        manifest_mime_type: Option<&str>,
        preserve_text_manifest: bool,
    ) -> Option<ResolvedStreamSource> {
        if manifest_b64.starts_with("https://") {
            return Some(ResolvedStreamSource::Url(manifest_b64.to_string()));
        }

        let manifest_bytes = base64::engine::general_purpose::STANDARD
            .decode(manifest_b64)
            .ok()?;

        // JSON manifest (application/vnd.tidal.bts) includes direct URL list.
        let is_json_manifest = manifest_mime_type
            .map(|m| m.contains("vnd.tidal.bts"))
            .unwrap_or(true);
        if is_json_manifest {
            if let Ok(manifest) = serde_json::from_slice::<ManifestJson>(&manifest_bytes) {
                if let Some(url) = manifest.urls.and_then(|u| u.into_iter().next()) {
                    return Some(ResolvedStreamSource::Url(url));
                }
            }
        }

        if !preserve_text_manifest {
            return None;
        }

        let text = String::from_utf8(manifest_bytes).ok()?;
        let file_extension = match manifest_mime_type.unwrap_or_default() {
            mime if mime.contains("dash+xml") => "mpd",
            mime if mime.contains("mpegurl") || mime.contains("x-mpegurl") => "m3u8",
            _ => "mpd",
        };
        Some(ResolvedStreamSource::Manifest {
            contents: text,
            file_extension: file_extension.to_string(),
        })
    }

    async fn request_playback_info(
        &self,
        track_id: &str,
        audioquality: &str,
    ) -> Result<Option<PlaybackInfoResponse>, Box<dyn std::error::Error>> {
        let country = if self.country_code.is_empty() {
            "US"
        } else {
            &self.country_code
        };

        let resp = self
            .client
            .get(format!(
                "{API_URL}/tracks/{track_id}/playbackinfopostpaywall"
            ))
            .bearer_auth(&self.access_token)
            .query(&[
                ("audioquality", audioquality),
                ("playbackmode", "STREAM"),
                ("assetpresentation", "FULL"),
                ("countryCode", country),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let info: PlaybackInfoResponse = resp.json().await?;
        Ok(Some(info))
    }

    async fn get_stream_url(
        &self,
        track_id: &str,
        max_stream_quality: MaxStreamQuality,
    ) -> Result<Option<ResolvedStream>, Box<dyn std::error::Error>> {
        let desired_quality = max_stream_quality.tidal_audioquality();
        if let Some(info) = self
            .request_playback_info(track_id, desired_quality)
            .await?
        {
            if let Some(manifest_b64) = info.manifest {
                if let Some(source) = Self::extract_stream_source_from_manifest(
                    &manifest_b64,
                    info.manifest_mime_type.as_deref(),
                    max_stream_quality == MaxStreamQuality::HiRes,
                ) {
                    return Ok(Some(ResolvedStream {
                        source,
                        quality_label: Some(max_stream_quality.tidal_quality_label().to_string()),
                    }));
                }
            }
        }

        // Some HI_RES responses provide manifests we can't directly play.
        // Retry at LOSSLESS as a compatibility fallback.
        if max_stream_quality == MaxStreamQuality::HiRes {
            if let Some(info) = self.request_playback_info(track_id, "LOSSLESS").await? {
                if let Some(manifest_b64) = info.manifest {
                    if let Some(source) = Self::extract_stream_source_from_manifest(
                        &manifest_b64,
                        info.manifest_mime_type.as_deref(),
                        false,
                    ) {
                        return Ok(Some(ResolvedStream {
                            source,
                            quality_label: Some(
                                MaxStreamQuality::Cd.tidal_quality_label().to_string(),
                            ),
                        }));
                    }
                }
            }
        }

        Ok(None)
    }
}

// --- Token management (async) ---

async fn start_device_auth(
    client: &reqwest::Client,
) -> Result<DeviceAuthResponse, Box<dyn std::error::Error>> {
    let resp = client
        .post(format!("{AUTH_URL}/device_authorization"))
        .form(&[("client_id", CLIENT_ID), ("scope", "r_usr w_usr w_sub")])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Tidal device auth failed ({}): {}", status, body).into());
    }

    let device_resp = resp.json::<DeviceAuthResponse>().await?;
    Ok(device_resp)
}

async fn poll_token(
    client: &reqwest::Client,
    device_code: &str,
) -> Result<Option<TokenResponse>, Box<dyn std::error::Error>> {
    let resp = client
        .post(format!("{AUTH_URL}/token"))
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("scope", "r_usr w_usr w_sub"),
        ])
        .send()
        .await?;

    if resp.status().is_success() {
        let token: TokenResponse = resp.json().await?;
        return Ok(Some(token));
    }

    // Check if still pending (sub_status 1002) or expired
    let err: TokenErrorResponse = resp.json().await?;
    if err.sub_status == Some(1002) {
        // authorization_pending - keep polling
        Ok(None)
    } else {
        Err("Device authorization expired or was denied".into())
    }
}

async fn refresh_token(
    client: &reqwest::Client,
    refresh_tok: &str,
) -> Result<TokenResponse, Box<dyn std::error::Error>> {
    let resp = client
        .post(format!("{AUTH_URL}/token"))
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("refresh_token", refresh_tok),
            ("grant_type", "refresh_token"),
            ("scope", "r_usr w_usr w_sub"),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("Token refresh failed: {}", resp.status()).into());
    }

    let token: TokenResponse = resp.json().await?;
    Ok(token)
}

fn make_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// --- Public streaming source ---

pub struct TidalSource {
    config: TidalConfig,
    max_stream_quality: MaxStreamQuality,
    api_client: Option<TidalClient>,
    http_client: reqwest::Client,
    pending_device_code: Option<String>,
    last_poll: Option<Instant>,
    poll_interval: Duration,
}

impl TidalSource {
    pub fn new(config: TidalConfig, max_stream_quality: MaxStreamQuality) -> Self {
        Self {
            config,
            max_stream_quality,
            api_client: None,
            http_client: reqwest::Client::new(),
            pending_device_code: None,
            last_poll: None,
            poll_interval: POLL_INTERVAL,
        }
    }

    fn is_token_expired(&self) -> bool {
        self.config.token_expiry <= now_epoch()
    }

    fn try_refresh(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if self.config.refresh_token.is_empty() {
            return Ok(false);
        }

        let rt = make_runtime();
        let token_resp =
            rt.block_on(refresh_token(&self.http_client, &self.config.refresh_token))?;

        self.config.access_token = token_resp.access_token.clone();
        if let Some(rt) = token_resp.refresh_token {
            self.config.refresh_token = rt;
        }
        self.config.token_expiry = now_epoch() + token_resp.expires_in;

        if let Some(user) = &token_resp.user {
            if let Some(cc) = &user.country_code {
                self.config.country_code = cc.clone();
            }
        }

        self.api_client = Some(TidalClient::new(
            self.config.access_token.clone(),
            self.config.country_code.clone(),
        ));

        Ok(true)
    }

    fn ensure_valid_token(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.api_client.is_some() && !self.is_token_expired() {
            return Ok(());
        }

        if !self.config.access_token.is_empty() && self.is_token_expired() {
            self.try_refresh()?;
        }

        Ok(())
    }
}

impl StreamingService for TidalSource {
    fn name(&self) -> &str {
        "Tidal"
    }

    fn is_authenticated(&self) -> bool {
        self.api_client.is_some() && !self.is_token_expired()
    }

    fn authenticate(&mut self) -> Result<AuthStatus, Box<dyn std::error::Error>> {
        // Case 1: Have valid tokens
        if !self.config.access_token.is_empty() && !self.is_token_expired() {
            self.api_client = Some(TidalClient::new(
                self.config.access_token.clone(),
                self.config.country_code.clone(),
            ));
            return Ok(AuthStatus::Authenticated);
        }

        // Case 2: Have expired tokens - try refresh
        if !self.config.refresh_token.is_empty() {
            if self.try_refresh()? {
                return Ok(AuthStatus::Authenticated);
            }
        }

        // Case 3: No tokens - start device auth flow
        let rt = make_runtime();
        let device_resp = rt.block_on(start_device_auth(&self.http_client))?;

        let message = format!(
            "Visit {} and enter code: {}",
            device_resp.verification_uri, device_resp.user_code
        );
        self.pending_device_code = Some(device_resp.device_code);
        self.poll_interval = Duration::from_secs(device_resp.interval.max(5));
        self.last_poll = None;

        Ok(AuthStatus::PendingUserAction(message))
    }

    fn poll_auth(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let device_code = match &self.pending_device_code {
            Some(code) => code.clone(),
            None => return Ok(true),
        };

        // Respect polling interval
        if let Some(last) = self.last_poll {
            if last.elapsed() < self.poll_interval {
                return Ok(false);
            }
        }
        self.last_poll = Some(Instant::now());

        let rt = make_runtime();
        let result = rt.block_on(poll_token(&self.http_client, &device_code))?;

        match result {
            Some(token_resp) => {
                self.config.access_token = token_resp.access_token.clone();
                if let Some(rt) = token_resp.refresh_token {
                    self.config.refresh_token = rt;
                }
                self.config.token_expiry = now_epoch() + token_resp.expires_in;

                if let Some(user) = &token_resp.user {
                    if let Some(cc) = &user.country_code {
                        self.config.country_code = cc.clone();
                    }
                }

                self.api_client = Some(TidalClient::new(
                    self.config.access_token.clone(),
                    self.config.country_code.clone(),
                ));
                self.pending_device_code = None;

                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn persist_data(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn search(
        &mut self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<StreamTrack>, Box<dyn std::error::Error>> {
        self.ensure_valid_token()?;

        let client = self
            .api_client
            .as_ref()
            .ok_or("Not authenticated with Tidal")?;
        let rt = make_runtime();
        let tracks = rt.block_on(client.search(query, limit))?;
        Ok(tracks)
    }

    fn get_stream_url(
        &mut self,
        track_id: &str,
    ) -> Result<Option<ResolvedStream>, Box<dyn std::error::Error>> {
        self.ensure_valid_token()?;

        let client = self
            .api_client
            .as_ref()
            .ok_or("Not authenticated with Tidal")?;
        let rt = make_runtime();
        let url = rt.block_on(client.get_stream_url(track_id, self.max_stream_quality))?;
        Ok(url)
    }
}

impl std::fmt::Debug for TidalSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TidalSource")
            .field("authenticated", &self.is_authenticated())
            .field("has_pending_auth", &self.pending_device_code.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::TidalClient;
    use crate::sources::streaming::ResolvedStreamSource;

    #[test]
    fn hi_res_text_manifests_are_preserved_for_mpv() {
        let manifest = base64::engine::general_purpose::STANDARD
            .encode(r#"<MPD><BaseURL>https://audio.tidal.com/manifest.mpd</BaseURL></MPD>"#);

        let source = TidalClient::extract_stream_source_from_manifest(
            &manifest,
            Some("application/dash+xml"),
            true,
        );

        assert!(matches!(
            source,
            Some(ResolvedStreamSource::Manifest {
                file_extension,
                ..
            }) if file_extension == "mpd"
        ));
    }

    #[test]
    fn lossless_text_manifests_without_direct_urls_are_rejected() {
        let manifest = base64::engine::general_purpose::STANDARD
            .encode(r#"<MPD><BaseURL>https://audio.tidal.com/lossless.flac</BaseURL></MPD>"#);

        let source = TidalClient::extract_stream_source_from_manifest(
            &manifest,
            Some("application/dash+xml"),
            false,
        );

        assert!(source.is_none());
    }
}
