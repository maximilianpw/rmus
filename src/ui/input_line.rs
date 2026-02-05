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
