use ratatui::{style::Style, text::Span};

#[derive(Default, Debug)]
pub struct InputLine {
    pub active: bool,
    pub value: String,
    input_mode: bool,
    cursor: usize,
}

impl InputLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_input_mode(&self) -> bool {
        self.input_mode
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn enter_input_mode(&mut self) {
        self.input_mode = true;
        self.active = true;
        self.value.clear();
        self.cursor = 0;
    }

    pub fn set_value(&mut self, value: String) {
        self.cursor = value.len();
        self.value = value;
    }

    pub fn exit_input_mode(&mut self) {
        self.input_mode = false;
        self.active = false;
        self.value.clear();
        self.cursor = 0;
    }

    pub fn confirm_input(&mut self) {
        self.input_mode = false;
        self.active = false;
    }

    pub fn append_char(&mut self, c: char) {
        if self.active {
            self.value.insert(self.cursor, c);
            self.cursor += c.len_utf8();
        }
    }

    pub fn delete_char(&mut self) {
        if self.active && self.cursor > 0 {
            let prev = self.prev_char_boundary();
            self.value.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn delete_next_char(&mut self) {
        if self.active && self.cursor < self.value.len() {
            let next = self.next_char_boundary();
            self.value.drain(self.cursor..next);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary();
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.value.len() {
            self.cursor = self.next_char_boundary();
        }
    }

    pub fn move_cursor_to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.cursor = self.value.len();
    }

    pub fn display_spans(&self, show_cursor: bool, cursor_style: Style) -> Vec<Span<'static>> {
        if !show_cursor {
            return vec![Span::raw(self.value.clone())];
        }

        let (before_cursor, after_cursor) = self.value.split_at(self.cursor);
        vec![
            Span::raw(before_cursor.to_string()),
            Span::styled("_", cursor_style),
            Span::raw(after_cursor.to_string()),
        ]
    }

    fn prev_char_boundary(&self) -> usize {
        let mut idx = self.cursor - 1;
        while !self.value.is_char_boundary(idx) {
            idx -= 1;
        }
        idx
    }

    fn next_char_boundary(&self) -> usize {
        let mut idx = self.cursor + 1;
        while idx < self.value.len() && !self.value.is_char_boundary(idx) {
            idx += 1;
        }
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::InputLine;
    use ratatui::style::Style;

    #[test]
    fn home_end_and_delete_edit_at_cursor() {
        let mut input = InputLine::new();
        input.enter_input_mode();
        for c in "XLibrary".chars() {
            input.append_char(c);
        }

        input.move_cursor_to_start();
        input.delete_next_char();
        input.move_cursor_to_end();
        for c in " Source".chars() {
            input.append_char(c);
        }

        assert_eq!(input.value, "Library Source");
        assert_eq!(input.cursor(), "Library Source".len());
    }

    #[test]
    fn delete_next_char_respects_utf8_boundaries() {
        let mut input = InputLine::new();
        input.enter_input_mode();
        for c in "aéb".chars() {
            input.append_char(c);
        }
        input.move_cursor_left();
        input.move_cursor_left();

        input.delete_next_char();

        assert_eq!(input.value, "ab");
        assert_eq!(input.cursor(), "a".len());
    }

    #[test]
    fn display_spans_render_cursor_at_current_position() {
        let mut input = InputLine::new();
        input.enter_input_mode();
        for c in "album".chars() {
            input.append_char(c);
        }
        input.move_cursor_to_start();

        let rendered = input
            .display_spans(true, Style::default())
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect::<String>();

        assert_eq!(rendered, "_album");
    }
}
