use std::{fmt::Display, fs::DirEntry, path::PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Song {
    pub path: PathBuf,
    pub title: String,
    pub url: Option<String>,
}

impl Song {
    pub fn new(f: DirEntry) -> Self {
        Song {
            title: f.file_name().to_string_lossy().into_owned(),
            path: f.path(),
            url: None,
        }
    }

    pub fn from_url(title: String, url: String) -> Self {
        Song {
            title,
            path: PathBuf::new(),
            url: Some(url),
        }
    }

    pub fn is_stream(&self) -> bool {
        self.url.is_some()
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
