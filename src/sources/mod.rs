pub mod local;

pub trait MusicSource {
    fn get_albums(&self) -> Vec<String>;
}
