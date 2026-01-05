use base64::Engine;
use md5::{Digest, Md5};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::process::Command;

const BASE_URL: &str = "https://www.qobuz.com/api.json/0.2";
const QUALITY: u32 = 27; // 24-bit/192kHz FLAC

#[derive(Debug, Deserialize)]
struct LoginResponse {
    user_auth_token: Option<String>,
    user: Option<User>,
}

#[derive(Debug, Deserialize)]
struct User {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    tracks: Option<TracksContainer>,
}

#[derive(Debug, Deserialize)]
struct TracksContainer {
    items: Option<Vec<Track>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Track {
    id: u64,
    title: Option<String>,
    performer: Option<Performer>,
    album: Option<Album>,
}

#[derive(Debug, Deserialize, Clone)]
struct Performer {
    name: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct Album {
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    url: Option<String>,
}

struct QobuzPlayer {
    app_id: String,
    app_secret: String,
    token: Option<String>,
    client: reqwest::Client,
}

async fn get_app_id_and_secret() -> Result<(String, String), Box<dyn std::error::Error>> {
    println!("Fetching Qobuz app credentials...");

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

    // Validate secrets
    for secret in &decoded_secrets {
        let now = chrono::Utc::now();
        let ts = now.timestamp() as f64 + now.timestamp_subsec_micros() as f64 / 1_000_000.0;
        let sig_input = format!("trackgetFileUrlformat_id27intentstreamtrack_id1{ts}{secret}");
        let sig = format!("{:x}", Md5::digest(sig_input.as_bytes()));

        let url = format!(
            "{BASE_URL}/track/getFileUrl?format_id=27&intent=stream&track_id=1&request_ts={ts}&request_sig={sig}"
        );

        let resp = client.get(&url).header("X-App-Id", &app_id).send().await?;

        if resp.status().as_u16() != 400 {
            println!("Got app_id: {app_id}");
            return Ok((app_id, secret.clone()));
        }
    }

    Err("No valid secret found".into())
}

impl QobuzPlayer {
    fn new(app_id: String, app_secret: String) -> Self {
        Self {
            app_id,
            app_secret,
            token: None,
            client: reqwest::Client::new(),
        }
    }

    async fn login(&mut self, email: &str, password: &str) -> Result<bool, reqwest::Error> {
        let pwd_hash = format!("{:x}", Md5::digest(password.as_bytes()));

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
            let name = resp
                .user
                .and_then(|u| u.display_name)
                .unwrap_or_else(|| email.to_string());
            println!("Logged in as {name}");
            Ok(true)
        } else {
            println!("Login failed");
            Ok(false)
        }
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<Track>, reqwest::Error> {
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

        Ok(resp.tracks.and_then(|t| t.items).unwrap_or_default())
    }

    async fn get_stream_url(&self, track_id: &str) -> Result<Option<String>, reqwest::Error> {
        let ts = chrono::Utc::now().timestamp();
        let sig_input = format!(
            "trackgetFileUrlformat_id{QUALITY}intentstreamtrack_id{track_id}{ts}{}",
            self.app_secret
        );
        let sig = format!("{:x}", Md5::digest(sig_input.as_bytes()));

        let token = self.token.as_deref().unwrap_or("");

        let resp = self
            .client
            .get(format!("{BASE_URL}/track/getFileUrl"))
            .query(&[
                ("track_id", track_id),
                ("format_id", &QUALITY.to_string()),
                ("intent", "stream"),
                ("request_ts", &ts.to_string()),
                ("request_sig", &sig),
                ("app_id", &self.app_id),
                ("user_auth_token", token),
            ])
            .send()
            .await?;

        if resp.status().is_success() {
            let data: StreamResponse = resp.json().await?;
            Ok(data.url)
        } else {
            let text = resp.text().await?;
            println!("Stream error: {text}");
            Ok(None)
        }
    }

    fn play(&self, url: &str) {
        println!("Streaming... (q to stop, space to pause, left/right to seek)");
        let result = Command::new("mpv")
            .args(["--no-video", "--term-osd-bar", url])
            .status();
        if let Err(e) = result {
            eprintln!("Failed to run mpv: {e}");
        }
    }
}

fn read_line(prompt: &str) -> String {
    print!("{prompt}");
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Qobuz HiFi Player ===\n");

    let (app_id, app_secret) = get_app_id_and_secret().await?;
    let mut player = QobuzPlayer::new(app_id, app_secret);

    let email = read_line("Qobuz Email: ");
    let password = rpassword::prompt_password("Qobuz Password: ")?;

    if !player.login(&email, &password).await? {
        return Ok(());
    }

    loop {
        println!("\n[s]earch  [q]uit");
        let cmd = read_line("> ").to_lowercase();

        match cmd.as_str() {
            "q" => break,
            "s" => {
                let query = read_line("Search: ");
                let tracks = player.search(&query, 10).await?;

                if tracks.is_empty() {
                    println!("No results");
                    continue;
                }

                for (i, t) in tracks.iter().enumerate() {
                    let artist = t
                        .performer
                        .as_ref()
                        .and_then(|p| p.name.as_ref())
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown");
                    let title = t.title.as_deref().unwrap_or("Unknown");
                    let album = t
                        .album
                        .as_ref()
                        .and_then(|a| a.title.as_ref())
                        .map(|s| s.as_str())
                        .unwrap_or("");
                    println!("  [{i}] {artist} - {title} ({album})");
                }

                let pick = read_line("\nPlay #: ");
                if let Ok(idx) = pick.parse::<usize>() {
                    if idx < tracks.len() {
                        let track = &tracks[idx];
                        if let Some(url) = player.get_stream_url(&track.id.to_string()).await? {
                            let title = track.title.as_deref().unwrap_or("Unknown");
                            println!("\nPlaying: {title}");
                            player.play(&url);
                        } else {
                            println!("Could not get stream URL");
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
