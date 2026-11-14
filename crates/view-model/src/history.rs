//! A text box's undo and redo: a snapshot is taken before each edit, and a
//! run of typing is one edit. No toolkit in here.

use crate::text_buffer::TextBuffer;

/// What undo gives back: the text and where the caret was.
type Snapshot = (String, usize);

const LIMIT: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// A run of typing is going on: more typing adds no snapshot.
    pub typing: bool,
}

impl History {
    /// Forgets everything (the text was replaced from outside).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.typing = false;
    }

    /// Ends a run of typing (the caret moved, or something else was done).
    pub fn break_run(&mut self) {
        self.typing = false;
    }

    /// Called before an edit: the text as it is now can be come back to.
    pub fn record(&mut self, buf: &TextBuffer) {
        self.undo.push((buf.text().to_owned(), buf.head()));
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.typing = false;
    }

    /// Called before typing: the first of a run is recorded.
    pub fn record_typing(&mut self, buf: &TextBuffer) {
        if !self.typing {
            self.record(buf);
            self.typing = true;
        }
    }

    /// Back to the text before the last edit; false when there is none.
    pub fn undo(&mut self, buf: &mut TextBuffer) -> bool {
        self.typing = false;
        step(&mut self.undo, &mut self.redo, buf)
    }

    /// Forward again after an undo; false when there is none.
    pub fn redo(&mut self, buf: &mut TextBuffer) -> bool {
        self.typing = false;
        step(&mut self.redo, &mut self.undo, buf)
    }
}

fn step(from: &mut Vec<Snapshot>, to: &mut Vec<Snapshot>, buf: &mut TextBuffer) -> bool {
    while let Some((text, head)) = from.pop() {
        // A snapshot of the text as it is changes nothing: go on.
        if text == buf.text() {
            continue;
        }
        to.push((buf.text().to_owned(), buf.head()));
        buf.restore(&text, head);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_walk_the_edits() {
        let mut buf = TextBuffer::new("a");
        let mut h = History::default();
        h.record(&buf);
        buf.set_text("ab");
        h.record(&buf);
        buf.set_text("abc");
        assert!(h.undo(&mut buf));
        assert_eq!(buf.text(), "ab");
        assert!(h.undo(&mut buf));
        assert_eq!(buf.text(), "a");
        assert!(!h.undo(&mut buf));
        assert!(h.redo(&mut buf));
        assert_eq!(buf.text(), "ab");
        assert!(h.redo(&mut buf));
        assert_eq!(buf.text(), "abc");
        assert!(!h.redo(&mut buf));
    }

    #[test]
    fn a_run_of_typing_is_one_step_and_a_new_edit_drops_the_redos() {
        let mut buf = TextBuffer::new("");
        let mut h = History::default();
        for c in ["h", "i"] {
            h.record_typing(&buf);
            buf.insert(c);
        }
        assert!(h.undo(&mut buf));
        assert_eq!(buf.text(), "");
        assert!(h.redo(&mut buf));
        assert_eq!(buf.text(), "hi");
        h.undo(&mut buf);
        h.record(&buf);
        buf.set_text("x");
        assert!(!h.redo(&mut buf));
    }
}
