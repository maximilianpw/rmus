use std::{fmt::Display, fs::DirEntry, path::PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Song {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album_name: String,
    pub track_number: Option<u32>,
    pub duration_secs: Option<f64>,
    pub url: Option<String>,
    pub stream_manifest: Option<StreamManifest>,
    pub stream_quality: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StreamManifest {
    pub contents: String,
    pub file_extension: String,
}

impl Song {
    pub fn new(f: DirEntry) -> Self {
        Song {
            title: f.file_name().to_string_lossy().into_owned(),
            path: f.path(),
            ..Default::default()
        }
    }

    pub fn from_url(title: String, url: String, stream_quality: Option<String>) -> Self {
        Song {
            title,
            url: Some(url),
            stream_quality,
            ..Default::default()
        }
    }

    pub fn from_manifest(
        title: String,
        contents: String,
        file_extension: String,
        stream_quality: Option<String>,
    ) -> Self {
        Song {
            title,
            stream_manifest: Some(StreamManifest {
                contents,
                file_extension,
            }),
            stream_quality,
            ..Default::default()
        }
    }

    pub fn is_stream(&self) -> bool {
        self.url.is_some() || self.stream_manifest.is_some()
    }
}

impl Display for Song {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("title", &self.title)
            .field("path", &self.path)
            .finish()
    }
}
