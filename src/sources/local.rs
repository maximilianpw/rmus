use std::{fmt::Debug, fs, path::PathBuf};

use crate::{config::LocalSource, sources::MusicSource};

#[derive(Debug, Default)]
pub struct LocalFiles {
    pub name: String,
    pub files: Vec<LocalSource>,
}

impl MusicSource for LocalFiles {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn get_albums(&self) -> Vec<String> {
        self.files.iter().map(|f| f.name.clone()).collect()
    }

    fn get_songs_from_album(&self, path: PathBuf) -> Vec<String> {
        if let Ok(files) = fs::read_dir(path) {
            files
                .filter_map(|f| f.ok())
                .filter_map(|f| f.file_name().into_string().ok())
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl LocalFiles {
    pub fn new(name: String, files: Vec<LocalSource>) -> Box<Self> {
        Box::new(LocalFiles { name, files })
    }
}
