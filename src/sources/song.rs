use std::{fmt::Display, fs::DirEntry, path::PathBuf};

#[derive(Debug, Default, Clone)]
pub struct Song {
    pub path: PathBuf,
    pub title: String,
}

impl Song {
    pub fn new(f: DirEntry) -> Self {
        Song {
            title: f.file_name().to_string_lossy().into_owned(),
            path: f.path(),
        }
    }
}

impl Display for Song {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Song")
            .field("title", &self.title)
            .field("path", &self.path)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn song_title_handles_non_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::time::{SystemTime, UNIX_EPOCH};
        use std::{env, fs};

        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = env::temp_dir().join(format!("rmus-song-test-{ts}"));
        fs::create_dir_all(&dir).unwrap();

        let bad_name = OsStr::from_bytes(b"bad\xFFname");
        let path = dir.join(bad_name);
        if fs::write(&path, b"test").is_err() {
            let _ = fs::remove_dir_all(&dir);
            return;
        }

        let entry = fs::read_dir(&dir).unwrap().next().unwrap().unwrap();
        let song = Song::new(entry);
        assert!(!song.title.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }
}
