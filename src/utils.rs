use std::path::{Path, PathBuf};

use directories::BaseDirs;
use ratatui::layout::{Constraint, Layout, Rect};

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, center, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(center);

    center
}

pub fn track_count_label(count: usize) -> String {
    if count == 1 {
        "1 track".to_string()
    } else {
        format!("{count} tracks")
    }
}

pub fn expand_home_path(path: &Path) -> PathBuf {
    let path_text = path.to_string_lossy();
    let Some(home) = BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) else {
        return path.to_path_buf();
    };

    if path_text == "~" {
        return home;
    }

    path_text
        .strip_prefix("~/")
        .or_else(|| path_text.strip_prefix("~\\"))
        .map_or_else(|| path.to_path_buf(), |rest| home.join(rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_count_label_uses_singular_for_one_track() {
        assert_eq!(track_count_label(1), "1 track");
    }

    #[test]
    fn track_count_label_uses_plural_for_zero_and_many_tracks() {
        assert_eq!(track_count_label(0), "0 tracks");
        assert_eq!(track_count_label(2), "2 tracks");
    }

    #[test]
    fn expand_home_path_preserves_non_home_paths() {
        let path = PathBuf::from("music");

        assert_eq!(expand_home_path(&path), path);
    }
}
