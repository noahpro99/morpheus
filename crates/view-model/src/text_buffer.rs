//! The text and selection of an editable box, with no UI toolkit in it:
//! typing, deleting, moving the caret by character, word and line, and
//! selecting. Offsets are bytes of the text and always on character
//! boundaries. A GPUI view (`input.rs`) draws it and maps keys and the mouse
//! to these calls; a native mobile UI could do the same.

use std::ops::Range;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextBuffer {
    text: String,
    /// Where a selection started (equal to `head` when nothing is selected).
    anchor: usize,
    /// The caret: the moving end of the selection.
    head: usize,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl TextBuffer {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            anchor: text.len(),
            head: text.len(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The caret.
    pub fn head(&self) -> usize {
        self.head
    }

    /// The selected range (empty when only the caret shows).
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    /// Replaces everything; the caret goes to the end.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_owned();
        self.anchor = self.text.len();
        self.head = self.text.len();
    }

    fn clamp(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// The caret to `offset`, or (extending) only its moving end.
    pub fn move_to(&mut self, offset: usize, extend: bool) {
        self.head = self.clamp(offset);
        if !extend {
            self.anchor = self.head;
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.head = self.text.len();
    }

    pub fn select_range(&mut self, range: Range<usize>) {
        self.anchor = self.clamp(range.start);
        self.head = self.clamp(range.end);
    }

    /// The character boundary before `offset`.
    pub fn prev_boundary(&self, offset: usize) -> usize {
        self.text[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    /// The character boundary after `offset`.
    pub fn next_boundary(&self, offset: usize) -> usize {
        self.text[offset..]
            .chars()
            .next()
            .map_or(offset, |c| offset + c.len_utf8())
    }

    /// The start of the word before `offset` (Ctrl+Left).
    pub fn prev_word(&self, offset: usize) -> usize {
        let mut at = offset;
        let before = &self.text[..offset];
        let mut chars = before.char_indices().rev().peekable();
        while let Some(&(i, c)) = chars.peek() {
            if c.is_whitespace() {
                at = i;
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(_, first)) = chars.peek() else {
            return at;
        };
        let class = is_word(first);
        for (i, c) in chars {
            if c.is_whitespace() || is_word(c) != class {
                break;
            }
            at = i;
        }
        at
    }

    /// The end of the word after `offset` (Ctrl+Right).
    pub fn next_word(&self, offset: usize) -> usize {
        let mut at = offset;
        let mut chars = self.text[offset..].chars().peekable();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                at += c.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let Some(&first) = chars.peek() else {
            return at;
        };
        let class = is_word(first);
        for c in chars {
            if c.is_whitespace() || is_word(c) != class {
                break;
            }
            at += c.len_utf8();
        }
        at
    }

    /// The start of the line (up to a `\n`) holding `offset`.
    pub fn line_start(&self, offset: usize) -> usize {
        self.text[..offset].rfind('\n').map_or(0, |i| i + 1)
    }

    /// The end of the line (before its `\n`) holding `offset`.
    pub fn line_end(&self, offset: usize) -> usize {
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i)
    }

    /// The word (or the run of spaces, or the one symbol) at `offset`: what a
    /// double click selects. A word beside the caret wins over what is on
    /// the other side.
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        let offset = self.clamp(offset);
        let after = self.text[offset..].chars().next();
        let before = self.text[..offset].chars().next_back();
        let (probe, at) = match (after, before) {
            (Some(a), _) if is_word(a) => (a, offset),
            (_, Some(b)) if is_word(b) => (b, offset - b.len_utf8()),
            (Some(a), _) => (a, offset),
            (None, Some(b)) => (b, offset - b.len_utf8()),
            (None, None) => return offset..offset,
        };
        let same = |c: char| {
            if is_word(probe) {
                is_word(c)
            } else if probe.is_whitespace() {
                c.is_whitespace() && c != '\n'
            } else {
                !is_word(c) && !c.is_whitespace()
            }
        };
        let mut start = at;
        for (i, c) in self.text[..at].char_indices().rev() {
            if !same(c) {
                break;
            }
            start = i;
        }
        let mut end = at + probe.len_utf8();
        for c in self.text[end..].chars() {
            if !same(c) {
                break;
            }
            end += c.len_utf8();
        }
        start..end
    }

    /// Typing: `text` replaces the selection, or goes in at the caret.
    pub fn insert(&mut self, text: &str) {
        let range = self.selection();
        self.text.replace_range(range.clone(), text);
        self.head = range.start + text.len();
        self.anchor = self.head;
    }

    /// Removes the selection (if any) and returns what it held.
    pub fn take_selection(&mut self) -> String {
        let range = self.selection();
        let taken = self.text[range.clone()].to_owned();
        self.text.replace_range(range.clone(), "");
        self.head = range.start;
        self.anchor = range.start;
        taken
    }

    fn delete_range(&mut self, range: Range<usize>) {
        self.text.replace_range(range.clone(), "");
        self.head = range.start;
        self.anchor = range.start;
    }

    /// Backspace: the selection, else the character before the caret (or the
    /// word, with `by_word`). Whether anything went.
    pub fn backspace(&mut self, by_word: bool) -> bool {
        if self.has_selection() {
            self.take_selection();
            return true;
        }
        if self.head == 0 {
            return false;
        }
        let start = if by_word {
            self.prev_word(self.head)
        } else {
            self.prev_boundary(self.head)
        };
        self.delete_range(start..self.head);
        true
    }

    /// Delete: the selection, else the character after the caret (or the word).
    pub fn delete(&mut self, by_word: bool) -> bool {
        if self.has_selection() {
            self.take_selection();
            return true;
        }
        if self.head >= self.text.len() {
            return false;
        }
        let end = if by_word {
            self.next_word(self.head)
        } else {
            self.next_boundary(self.head)
        };
        self.delete_range(self.head..end);
        true
    }

    /// Left: the selection's start if there is one (unless extending).
    pub fn left(&mut self, extend: bool, by_word: bool) {
        if self.has_selection() && !extend {
            self.move_to(self.selection().start, false);
        } else {
            let to = if by_word {
                self.prev_word(self.head)
            } else {
                self.prev_boundary(self.head)
            };
            self.move_to(to, extend);
        }
    }

    pub fn right(&mut self, extend: bool, by_word: bool) {
        if self.has_selection() && !extend {
            self.move_to(self.selection().end, false);
        } else {
            let to = if by_word {
                self.next_word(self.head)
            } else {
                self.next_boundary(self.head)
            };
            self.move_to(to, extend);
        }
    }

    /// Home: the start of the caret's line (the whole text with `whole`).
    pub fn home(&mut self, extend: bool, whole: bool) {
        let to = if whole { 0 } else { self.line_start(self.head) };
        self.move_to(to, extend);
    }

    pub fn end(&mut self, extend: bool, whole: bool) {
        let to = if whole {
            self.text.len()
        } else {
            self.line_end(self.head)
        };
        self.move_to(to, extend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_goes_in_at_the_caret_and_replaces_a_selection() {
        let mut b = TextBuffer::new("hello");
        b.insert(" world");
        assert_eq!(b.text(), "hello world");
        b.select_range(0..5);
        b.insert("bye");
        assert_eq!(b.text(), "bye world");
        assert_eq!(b.head(), 3);
    }

    #[test]
    fn deleting_works_by_character_and_by_word() {
        let mut b = TextBuffer::new("one two three");
        assert!(b.backspace(false));
        assert_eq!(b.text(), "one two thre");
        assert!(b.backspace(true));
        assert_eq!(b.text(), "one two ");
        assert!(b.backspace(true));
        assert_eq!(b.text(), "one ");
        b.move_to(0, false);
        assert!(!b.backspace(false));
        assert!(b.delete(true));
        assert_eq!(b.text(), " ");
        assert!(b.delete(false));
        assert!(!b.delete(false));
        assert!(b.is_empty());
    }

    #[test]
    fn characters_of_any_width_are_whole() {
        let mut b = TextBuffer::new("a🔥é");
        b.left(false, false);
        assert_eq!(b.head(), "a🔥".len());
        b.left(false, false);
        assert_eq!(b.head(), 1);
        b.right(false, false);
        assert_eq!(b.head(), 5);
        assert!(b.backspace(false));
        assert_eq!(b.text(), "aé");
        b.move_to(2, false);
        assert_eq!(b.head(), 1, "an offset inside a character moves back to it");
    }

    #[test]
    fn selecting_extends_from_where_it_started() {
        let mut b = TextBuffer::new("hello world");
        b.move_to(0, false);
        b.right(true, false);
        b.right(true, false);
        assert_eq!(b.selected_text(), "he");
        b.left(true, false);
        assert_eq!(b.selected_text(), "h");
        b.right(true, true);
        assert_eq!(b.selected_text(), "hello");
        // Without shift an arrow collapses to the selection's edge.
        b.left(false, false);
        assert!(!b.has_selection());
        assert_eq!(b.head(), 0);
        b.select_all();
        assert_eq!(b.selected_text(), "hello world");
        b.right(false, false);
        assert_eq!(b.head(), 11);
    }

    #[test]
    fn lines_split_at_newlines() {
        let mut b = TextBuffer::new("ab\ncd\nef");
        b.move_to(4, false);
        assert_eq!((b.line_start(4), b.line_end(4)), (3, 5));
        b.home(false, false);
        assert_eq!(b.head(), 3);
        b.end(false, false);
        assert_eq!(b.head(), 5);
        b.home(true, true);
        assert_eq!(b.selected_text(), "ab\ncd");
        b.insert("\n");
        assert_eq!(b.text(), "\n\nef");
    }

    #[test]
    fn a_double_click_takes_the_word() {
        let b = TextBuffer::new("hey @bob, ok");
        assert_eq!(&b.text()[b.word_at(1)], "hey");
        assert_eq!(&b.text()[b.word_at(3)], "hey");
        assert_eq!(&b.text()[b.word_at(4)], "@");
        assert_eq!(&b.text()[b.word_at(6)], "bob");
        assert_eq!(&b.text()[b.word_at(12)], "ok");
        assert_eq!(b.word_at(0), 0..3);
    }

    #[test]
    fn cutting_returns_the_selection() {
        let mut b = TextBuffer::new("cut this out");
        b.select_range(4..8);
        assert_eq!(b.take_selection(), "this");
        assert_eq!(b.text(), "cut  out");
        assert_eq!(b.head(), 4);
        assert_eq!(b.take_selection(), "");
    }
}
