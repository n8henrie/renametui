#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn insert(&mut self, character: char) {
        self.value.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub(crate) fn backspace(&mut self) -> bool {
        let Some(previous) = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
        else {
            return false;
        };

        self.value.drain(previous..self.cursor);
        self.cursor = previous;
        true
    }

    pub(crate) fn delete(&mut self) -> bool {
        let Some(character) = self.value[self.cursor..].chars().next() else {
            return false;
        };

        let end = self.cursor + character.len_utf8();
        self.value.drain(self.cursor..end);
        true
    }

    pub(crate) fn move_left(&mut self) {
        if let Some(previous) = self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
        {
            self.cursor = previous;
        };
    }

    pub(crate) fn move_right(&mut self) {
        if let Some(character) = self.value[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        };
    }

    pub(crate) fn home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn end(&mut self) {
        self.cursor = self.value.len();
    }

    pub(crate) fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::TextInput;

    #[test]
    fn editing_respects_unicode_boundaries() {
        let mut input = TextInput::default();
        input.insert_str("a🦀c");
        input.move_left();
        input.backspace();
        input.insert('b');

        assert_eq!(input.value(), "abc");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        let mut input = TextInput::default();
        input.insert_str("a🦀c");
        input.home();
        input.move_right();

        assert!(input.delete());
        assert_eq!(input.value(), "ac");
    }

    #[test]
    fn backspace_and_delete_are_noops_at_the_edges() {
        let mut input = TextInput::default();
        assert!(!input.backspace());
        assert!(!input.delete());
    }
}
