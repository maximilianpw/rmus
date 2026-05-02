use std::{fmt::Debug, path::PathBuf};

use crate::sources::song::Song;

pub mod local;
pub mod qobuz;
pub mod song;
pub mod streaming;
pub mod tidal;

pub trait MusicSource {
    fn name(&self) -> String;
    fn get_albums(&self) -> Vec<String>;
    fn get_album_path(&self, index: usize) -> Option<PathBuf>;
    fn get_songs_from_album(&self, path: PathBuf) -> Vec<Song>;
}

impl Debug for dyn MusicSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Music Source: name = {}", self.name())
    }
}

/// Placeholder source for streaming service tabs in the left panel.
/// Returns no albums — streaming services use search instead.
pub struct StreamingTab {
    service_name: String,
}

impl StreamingTab {
    pub fn boxed(name: &str) -> Box<dyn MusicSource> {
        Box::new(Self {
            service_name: name.to_string(),
        })
    }
}

impl MusicSource for StreamingTab {
    fn name(&self) -> String {
        self.service_name.clone()
    }

    fn get_albums(&self) -> Vec<String> {
        vec![]
    }

    fn get_album_path(&self, _index: usize) -> Option<PathBuf> {
        None
    }

    fn get_songs_from_album(&self, _path: PathBuf) -> Vec<Song> {
        vec![]
    }
}
