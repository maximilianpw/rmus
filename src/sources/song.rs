use std::{fmt::Display, fs::DirEntry, path::PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Song {
    pub path: PathBuf,
    pub title: String,
}

impl Song {
    pub fn new(f: DirEntry) -> Self {
        Song {
            title: f
                .file_name()
                .into_string()
                .expect("could not get title from path"),
            path: f.path(),
        }
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
