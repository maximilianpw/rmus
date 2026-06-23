use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Tabs},
    Frame,
};

use crate::players::{PlaybackInfo, PlaybackState, RepeatMode, ShuffleMode};
use crate::sources::song::Song;
use crate::ui::theme;

pub fn handle_focused_border_style(is_focused: bool) -> Style {
    theme::focused_border_style(is_focused)
}

pub fn selected_row_style() -> Style {
    theme::selected_row_style()
}

pub fn tabs_from_strings<'a>(
    tabs_items: &'a [String],
    selected_tab_index: usize,
    is_focused: bool,
) -> Tabs<'a> {
    let border_style = handle_focused_border_style(is_focused);
    Tabs::new(tabs_items.to_owned())
        .block(
            Block::bordered()
                .title("Sources")
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .select(selected_tab_index)
        .highlight_style(theme::accent_bold_style())
}

pub fn now_playing_widget(info: &PlaybackInfo, is_focused: bool, frame: &mut Frame, area: Rect) {
    let border_style = handle_focused_border_style(is_focused);

    let title = match &info.current_song {
        Some(song) => now_playing_title(song),
        None => "Not Playing".to_string(),
    };

    let block = Block::bordered()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    let mut row_y = inner.y;
    let metadata_row_limit = inner.height.saturating_sub(3) as usize;
    if let Some(song) = &info.current_song {
        for line in now_playing_metadata_lines(song)
            .into_iter()
            .take(metadata_row_limit)
        {
            let row = Rect::new(inner.x, row_y, inner.width, 1);
            frame.render_widget(Paragraph::new(line), row);
            row_y += 1;
        }
    }

    // Status line
    let status = match info.state {
        PlaybackState::Playing => Span::styled("Playing", theme::success_style()),
        PlaybackState::Paused => Span::styled("Paused", theme::warning_style()),
        PlaybackState::Stopped => Span::styled("Stopped", theme::muted_style()),
    };
    let quality = info
        .current_song
        .as_ref()
        .and_then(|song| song.stream_quality.as_ref())
        .map(|quality| Span::raw(format!(" | Quality: {}", quality)));
    let volume = Span::raw(format!(" | Vol: {}%", info.volume));
    let mut status_parts = vec![status];
    if let Some(quality) = quality {
        status_parts.push(quality);
    }
    status_parts.push(volume);
    if info.shuffle == ShuffleMode::On {
        status_parts.push(Span::styled(" | Shuffle", theme::secondary_style()));
    }
    if info.repeat != RepeatMode::Off {
        status_parts.push(Span::styled(
            format!(" | Repeat: {}", info.repeat.label()),
            theme::info_style(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(status_parts)),
        Rect::new(inner.x, row_y, inner.width, 1),
    );
    row_y += 1;

    // Progress bar
    let progress = if info.duration > 0.0 {
        (info.position / info.duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let gauge = Gauge::default()
        .ratio(progress)
        .gauge_style(theme::accent_style());
    frame.render_widget(gauge, Rect::new(inner.x, row_y, inner.width, 1));
    row_y += 1;

    let time_str = format!(
        "{} / {}",
        playback_time_label(info.position),
        playback_time_label(info.duration)
    );
    frame.render_widget(
        Paragraph::new(time_str).alignment(Alignment::Center),
        Rect::new(inner.x, row_y, inner.width, 1),
    );
}

fn playback_time_label(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00".to_string();
    }

    let total_seconds = seconds.floor() as u64;
    let secs = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let mins = total_minutes % 60;
    let hours = total_minutes / 60;

    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

fn now_playing_title(song: &Song) -> String {
    let title = song.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }

    let path = song.path.to_string_lossy();
    if !path.is_empty() {
        return path.into_owned();
    }

    "Unknown Track".to_string()
}

fn now_playing_metadata_lines(song: &Song) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let artist = song.artist.trim();
    if !artist.is_empty() {
        lines.push(metadata_line("Artist", artist));
    }

    let album = song.album_name.trim();
    if !album.is_empty() {
        lines.push(metadata_line("Album", album));
    }

    if let Some(disc_number) = song.disc_number {
        lines.push(metadata_line("Disc", &disc_number.to_string()));
    }

    if let Some(track_number) = song.track_number {
        lines.push(metadata_line("Track", &track_number.to_string()));
    }

    if let Some(source) = song
        .stream_service
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
    {
        lines.push(metadata_line("Source", source));
    }

    lines
}

fn metadata_line(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), theme::muted_style()),
        Span::raw(value.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    use crate::{
        players::{PlaybackInfo, PlaybackState},
        sources::song::Song,
        ui::theme,
    };

    use super::now_playing_widget;

    fn extract_buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    text.push_str(cell.symbol());
                }
            }
            text.push('\n');
        }
        text
    }

    fn first_cell_fg_for_text(
        buffer: &ratatui::buffer::Buffer,
        needle: &str,
    ) -> ratatui::style::Color {
        for y in 0..buffer.area.height {
            let mut row = String::new();
            for x in 0..buffer.area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    row.push_str(cell.symbol());
                }
            }

            if let Some(offset) = row.find(needle) {
                return buffer
                    .cell((buffer.area.x + offset as u16, buffer.area.y + y))
                    .unwrap()
                    .fg;
            }
        }

        panic!("did not find {needle:?} in rendered buffer");
    }

    #[test]
    fn now_playing_widget_shows_stream_quality_label() {
        let backend = TestBackend::new(60, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PlaybackInfo {
            state: PlaybackState::Playing,
            current_song: Some(Song::from_url(
                "Artist - Track".to_string(),
                "https://example.com/track.flac".to_string(),
                Some("Lossless".to_string()),
            )),
            position: 30.0,
            duration: 120.0,
            volume: 50,
            last_error: None,
            ..Default::default()
        };

        let frame = terminal
            .draw(|frame| now_playing_widget(&info, false, frame, frame.area()))
            .unwrap();
        let text = extract_buffer_text(frame.buffer);

        assert!(text.contains("Quality: Lossless"));
    }

    #[test]
    fn now_playing_status_uses_theme_contrast_colors() {
        let backend = TestBackend::new(60, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PlaybackInfo {
            state: PlaybackState::Playing,
            current_song: Some(Song {
                title: "Needle Drop".to_string(),
                ..Default::default()
            }),
            position: 30.0,
            duration: 120.0,
            volume: 50,
            last_error: None,
            ..Default::default()
        };

        let frame = terminal
            .draw(|frame| now_playing_widget(&info, false, frame, frame.area()))
            .unwrap();

        assert_eq!(
            first_cell_fg_for_text(frame.buffer, "Playing"),
            theme::SUCCESS
        );
    }

    #[test]
    fn now_playing_widget_shows_artist_and_album_metadata() {
        let backend = TestBackend::new(60, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PlaybackInfo {
            state: PlaybackState::Playing,
            current_song: Some(Song {
                title: "Blue Monday".to_string(),
                artist: "New Order".to_string(),
                album_name: "Substance".to_string(),
                path: PathBuf::from("/music/blue-monday.flac"),
                ..Default::default()
            }),
            position: 30.0,
            duration: 120.0,
            volume: 50,
            last_error: None,
            ..Default::default()
        };

        let frame = terminal
            .draw(|frame| now_playing_widget(&info, false, frame, frame.area()))
            .unwrap();
        let text = extract_buffer_text(frame.buffer);

        assert!(text.contains("Blue Monday"));
        assert!(text.contains("Artist: New Order"));
        assert!(text.contains("Album: Substance"));
    }

    #[test]
    fn now_playing_widget_shows_disc_and_track_metadata() {
        let backend = TestBackend::new(60, 9);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PlaybackInfo {
            state: PlaybackState::Playing,
            current_song: Some(Song {
                title: "Second Movement".to_string(),
                artist: "Performer".to_string(),
                album_name: "Collected Works".to_string(),
                disc_number: Some(2),
                track_number: Some(4),
                ..Default::default()
            }),
            position: 30.0,
            duration: 120.0,
            volume: 50,
            last_error: None,
            ..Default::default()
        };

        let frame = terminal
            .draw(|frame| now_playing_widget(&info, false, frame, frame.area()))
            .unwrap();
        let text = extract_buffer_text(frame.buffer);

        assert!(text.contains("Disc: 2"));
        assert!(text.contains("Track: 4"));
    }

    #[test]
    fn now_playing_widget_shows_stream_source_when_available() {
        let backend = TestBackend::new(60, 8);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PlaybackInfo {
            state: PlaybackState::Playing,
            current_song: Some(Song {
                title: "Night Drive".to_string(),
                artist: "Streaming Artist".to_string(),
                album_name: "Late Set".to_string(),
                stream_service: Some("Qobuz".to_string()),
                stream_quality: Some("Hi-Res".to_string()),
                ..Default::default()
            }),
            position: 30.0,
            duration: 120.0,
            volume: 50,
            last_error: None,
            ..Default::default()
        };

        let frame = terminal
            .draw(|frame| now_playing_widget(&info, false, frame, frame.area()))
            .unwrap();
        let text = extract_buffer_text(frame.buffer);

        assert!(text.contains("Source: Qobuz"));
        assert!(text.contains("Quality: Hi-Res"));
    }

    #[test]
    fn now_playing_widget_formats_hour_long_durations() {
        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        let info = PlaybackInfo {
            state: PlaybackState::Playing,
            current_song: Some(Song {
                title: "Long Track".to_string(),
                ..Default::default()
            }),
            position: 3661.0,
            duration: 4200.0,
            volume: 50,
            last_error: None,
            ..Default::default()
        };

        let frame = terminal
            .draw(|frame| now_playing_widget(&info, false, frame, frame.area()))
            .unwrap();
        let text = extract_buffer_text(frame.buffer);

        assert!(text.contains("1:01:01 / 1:10:00"));
    }
}
