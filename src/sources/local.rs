use std::{fmt::Debug, path::PathBuf};

use crate::sources::MusicSource;

#[derive(Debug, Default)]
pub struct LocalFiles {
    pub paths: Vec<PathBuf>,
}

impl MusicSource for LocalFiles {
    fn name(&self) -> String {
        "Local".to_string()
    }

    fn get_albums(&self) -> Vec<String> {
        let mut albums: Vec<String> = self
            .paths
            .iter()
            .map(|path| format!("{}", path.display()))
            .collect();
        albums.extend(vec![
            "Song 1 - Artist A".to_string(),
            "Song 2 - Artist B".to_string(),
            "Song 3 - Artist C".to_string(),
            "Song 4 - Artist D".to_string(),
        ]);
        albums
    }
}

impl LocalFiles {
    pub fn new() -> Box<Self> {
        Box::new(LocalFiles { paths: vec![] })
    }
}
