use std::{fmt::Display, fs::DirEntry, path::PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Song {
    pub path: PathBuf,
    pub title: String,
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
            url: None,
            stream_manifest: None,
            stream_quality: None,
        }
    }

    pub fn from_url(title: String, url: String, stream_quality: Option<String>) -> Self {
        Song {
            title,
            path: PathBuf::new(),
            url: Some(url),
            stream_manifest: None,
            stream_quality,
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
            path: PathBuf::new(),
            url: None,
            stream_manifest: Some(StreamManifest {
                contents,
                file_extension,
            }),
            stream_quality,
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
