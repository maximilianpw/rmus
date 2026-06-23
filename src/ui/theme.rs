use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::LightCyan;
pub const FOCUS: Color = Color::LightCyan;
pub const SELECTED_FG: Color = Color::Black;
pub const SELECTED_BG: Color = Color::LightCyan;
pub const MUTED: Color = Color::Gray;
pub const DIVIDER: Color = Color::Gray;
pub const SECTION: Color = Color::LightYellow;
pub const SUCCESS: Color = Color::LightGreen;
pub const WARNING: Color = Color::LightYellow;
pub const ERROR: Color = Color::LightRed;
pub const INFO: Color = Color::LightBlue;
pub const SECONDARY: Color = Color::LightMagenta;
pub const CURRENT: Color = Color::LightGreen;

pub fn default_style() -> Style {
    Style::default()
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

    #[test]
    fn selected_row_uses_explicit_high_contrast_colors() {
        let style = selected_row_style();

        assert_eq!(style.fg, Some(SELECTED_FG));
        assert_eq!(style.bg, Some(SELECTED_BG));
        assert!(style.add_modifier.contains(Modifier::BOLD));
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
