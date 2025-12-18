use crate::sources::MusicSource;

struct LocalFiles {}

impl MusicSource for LocalFiles {
    fn get_albums(&self) -> Vec<String> {
        todo!()
    }
}
