use std::{fmt::Display, fs::DirEntry, path::PathBuf};

use lofty::{
    file::{AudioFile, TaggedFileExt},
    tag::Accessor,
};

#[derive(Debug, Default, Clone)]
pub struct Song {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album_name: String,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub duration_secs: Option<f64>,
    pub url: Option<String>,
    pub stream_manifest: Option<StreamManifest>,
    pub stream_quality: Option<String>,
    pub stream_service: Option<String>,
    pub stream_track_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StreamManifest {
    pub contents: String,
    pub file_extension: String,
}

impl Song {
    pub fn new(f: DirEntry) -> Self {
        Self::from_path(f.path())
    }

    pub fn from_path(path: PathBuf) -> Self {
        let fallback_title = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());

        let mut song = Song {
            title: fallback_title,
            path: path.clone(),
            ..Default::default()
        };

        let Ok(tagged_file) = lofty::read_from_path(&path) else {
            return song;
        };

        if let Some(tag) = tagged_file
            .primary_tag()
            .or_else(|| tagged_file.first_tag())
        {
            if let Some(title) = tag.title() {
                song.title = title.into_owned();
            }
            if let Some(artist) = tag.artist() {
                song.artist = artist.into_owned();
            }
            if let Some(album) = tag.album() {
                song.album_name = album.into_owned();
            }
            song.disc_number = tag.disk();
            song.track_number = tag.track();
        }

        let duration = tagged_file.properties().duration();
        if duration.as_secs_f64() > 0.0 {
            song.duration_secs = Some(duration.as_secs_f64());
        }

        song
    }

    pub fn from_url(title: String, url: String, stream_quality: Option<String>) -> Self {
        Song {
            title,
            url: Some(url),
            stream_quality,
            ..Default::default()
        }
    }

    pub fn from_manifest(
        title: String,
        contents: String,
        file_extension: String,
        stream_quality: Option<String>,
    ) -> Self {
        Song {
            title,
            stream_manifest: Some(StreamManifest {
                contents,
                file_extension,
            }),
            stream_quality,
            ..Default::default()
        }
    }

    pub fn is_stream(&self) -> bool {
        self.url.is_some() || self.stream_manifest.is_some()
    }

    pub fn has_stream_reference(&self) -> bool {
        self.stream_service.is_some() && self.stream_track_id.is_some()
    }
}

impl Display for Song {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.artist.trim(), self.title.trim()) {
            ("", "") => write!(f, "{}", self.path.to_string_lossy()),
            ("", title) => write!(f, "{title}"),
            (artist, "") => write!(f, "{artist}"),
            (artist, title) => write!(f, "{artist} - {title}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_artist_context_when_available() {
        let song = Song {
            title: "Ceremony".to_string(),
            artist: "New Order".to_string(),
            ..Default::default()
        };

        assert_eq!(song.to_string(), "New Order - Ceremony");
    }

    #[test]
    fn display_falls_back_to_path_when_title_is_blank() {
        let song = Song {
            path: PathBuf::from("/music/untagged.flac"),
            ..Default::default()
        };

        assert_eq!(song.to_string(), "/music/untagged.flac");
    }
}
