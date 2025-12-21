use std::{fmt::Debug, path::PathBuf};

pub mod local;

pub trait MusicSource {
    // provides the lable for the tag
    fn name(&self) -> String;
    fn get_albums(&self) -> Vec<String>;
    fn get_songs_from_album(&self, name: PathBuf) -> Vec<String>;
}

impl Debug for dyn MusicSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Music Source: name = {}", self.name())
    }
}
