use std::{fmt::Debug, path::PathBuf};

use crate::sources::song::Song;

pub mod local;
pub mod song;

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
