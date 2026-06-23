use ratatui::style::{Color, Modifier, Style};

pub const BACKGROUND: Color = Color::Rgb(8, 10, 13);
pub const FOREGROUND: Color = Color::Rgb(238, 242, 247);
pub const ACCENT: Color = Color::Rgb(83, 234, 201);
pub const FOCUS: Color = Color::Rgb(245, 198, 96);
pub const SELECTED_FG: Color = BACKGROUND;
pub const SELECTED_BG: Color = FOREGROUND;
pub const MUTED: Color = Color::Rgb(150, 158, 171);
pub const DIVIDER: Color = Color::Rgb(120, 132, 150);
pub const SECTION: Color = Color::Rgb(255, 212, 107);
pub const SUCCESS: Color = Color::Rgb(99, 230, 128);
pub const WARNING: Color = Color::Rgb(255, 184, 77);
pub const ERROR: Color = Color::Rgb(255, 107, 107);
pub const INFO: Color = Color::Rgb(131, 171, 255);
pub const SECONDARY: Color = Color::Rgb(217, 176, 255);
pub const CURRENT: Color = Color::Rgb(136, 255, 180);

pub fn default_style() -> Style {
    Style::default().fg(FOREGROUND).bg(BACKGROUND)
}

pub fn focused_border_style(is_focused: bool) -> Style {
    if is_focused {
        Style::default().fg(FOCUS).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIVIDER)
    }
}

pub fn selected_row_style() -> Style {
    Style::default()
        .fg(SELECTED_FG)
        .bg(SELECTED_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn accent_style() -> Style {
    Style::default().fg(ACCENT)
}

pub fn accent_bold_style() -> Style {
    accent_style().add_modifier(Modifier::BOLD)
}

pub fn section_style() -> Style {
    Style::default().fg(SECTION).add_modifier(Modifier::BOLD)
}

pub fn muted_style() -> Style {
    Style::default().fg(MUTED)
}

pub fn success_style() -> Style {
    Style::default().fg(SUCCESS)
}

pub fn warning_style() -> Style {
    Style::default().fg(WARNING)
}

pub fn error_style() -> Style {
    Style::default().fg(ERROR)
}

pub fn info_style() -> Style {
    Style::default().fg(INFO)
}

pub fn secondary_style() -> Style {
    Style::default().fg(SECONDARY)
}

pub fn current_style() -> Style {
    Style::default().fg(CURRENT).add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::*;

    const MIN_TEXT_CONTRAST: f64 = 4.5;

    fn rgb(color: Color) -> (u8, u8, u8) {
        match color {
            Color::Rgb(red, green, blue) => (red, green, blue),
            other => panic!("expected RGB color, got {other:?}"),
        }
    }

    fn linear_channel(value: u8) -> f64 {
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    fn relative_luminance(color: Color) -> f64 {
        let (red, green, blue) = rgb(color);
        0.2126 * linear_channel(red)
            + 0.7152 * linear_channel(green)
            + 0.0722 * linear_channel(blue)
    }

    fn contrast_ratio(first: Color, second: Color) -> f64 {
        let first = relative_luminance(first);
        let second = relative_luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn semantic_colors_have_readable_contrast_on_app_background() {
        for (name, color) in [
            ("foreground", FOREGROUND),
            ("accent", ACCENT),
            ("focus", FOCUS),
            ("muted", MUTED),
            ("divider", DIVIDER),
            ("section", SECTION),
            ("success", SUCCESS),
            ("warning", WARNING),
            ("error", ERROR),
            ("info", INFO),
            ("secondary", SECONDARY),
            ("current", CURRENT),
        ] {
            assert!(
                contrast_ratio(color, BACKGROUND) >= MIN_TEXT_CONTRAST,
                "{name} contrast is too low"
            );
        }
    }

    #[test]
    fn selected_row_uses_explicit_high_contrast_colors() {
        let style = selected_row_style();

        assert_eq!(style.fg, Some(SELECTED_FG));
        assert_eq!(style.bg, Some(SELECTED_BG));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(contrast_ratio(SELECTED_FG, SELECTED_BG) >= MIN_TEXT_CONTRAST);
    }

    #[test]
    fn default_style_sets_a_stable_terminal_theme_base() {
        let style = default_style();

        assert_eq!(style.fg, Some(FOREGROUND));
        assert_eq!(style.bg, Some(BACKGROUND));
        assert!(contrast_ratio(FOREGROUND, BACKGROUND) >= MIN_TEXT_CONTRAST);
    }

    #[test]
    fn focused_and_unfocused_borders_are_distinguishable() {
        let focused = focused_border_style(true);
        let unfocused = focused_border_style(false);

        assert_eq!(focused.fg, Some(FOCUS));
        assert_eq!(unfocused.fg, Some(DIVIDER));
        assert_ne!(focused.fg, unfocused.fg);
        assert!(focused.add_modifier.contains(Modifier::BOLD));
    }
}
