//! Selecting text with the pointer across the paragraphs of one message (or the
//! full-screen long message): where a click, a double click (a word), a triple
//! click (a paragraph), a drag and a shift-click put the two ends, which part of
//! each paragraph is selected, and the text a copy gives. Pure, with positions
//! as (paragraph, byte offset), so a view draws the highlight and finds the
//! positions however it lays text out.

use std::ops::Range;

/// A place in the text: a paragraph and a byte offset in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub para: usize,
    pub offset: usize,
}

impl Pos {
    pub fn new(para: usize, offset: usize) -> Self {
        Self { para, offset }
    }
}

/// How much one step of the selection covers: what the click count chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Char,
    Word,
    Para,
}

/// A selection: the unit first picked (a click's caret, a word, a paragraph) and
/// the end that moves, which a drag or a shift-click sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    origin: (Pos, Pos),
    unit: Unit,
    head: Pos,
}

/// The offset moved down to a character boundary within the text.
fn clamp(text: &str, offset: usize) -> usize {
    let mut at = offset.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[derive(PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Other,
}

fn class(c: char) -> Class {
    if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else if c.is_whitespace() {
        Class::Space
    } else {
        Class::Other
    }
}

/// The word around `offset`: the run of letters and digits (or of spaces, or of
/// punctuation) the offset is in or touches, a double click's selection. At the
/// very end of the text it is the run before.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    let at = clamp(text, offset);
    let after = text[at..].chars().next();
    let before = text[..at].chars().next_back();
    let Some(seed) = after.or(before) else {
        return at..at;
    };
    let kind = class(seed);
    let mut start = at;
    let mut end = at;
    if after.is_some() {
        for (i, c) in text[at..].char_indices() {
            if class(c) != kind {
                break;
            }
            end = at + i + c.len_utf8();
        }
        for (i, c) in text[..at].char_indices().rev() {
            if class(c) != kind {
                break;
            }
            start = i;
        }
    } else {
        for (i, c) in text[..at].char_indices().rev() {
            if class(c) != kind {
                break;
            }
            start = i;
        }
    }
    start..end
}

fn text_of<'a>(paras: &[&'a str], para: usize) -> &'a str {
    paras.get(para).copied().unwrap_or("")
}

impl Selection {
    /// A click: nothing selected yet, the anchor here.
    pub fn caret(at: Pos) -> Self {
        Self {
            origin: (at, at),
            unit: Unit::Char,
            head: at,
        }
    }

    /// A double click: the word at `at`.
    pub fn word(paras: &[&str], at: Pos) -> Self {
        let r = word_range(text_of(paras, at.para), at.offset);
        let (a, b) = (Pos::new(at.para, r.start), Pos::new(at.para, r.end));
        Self {
            origin: (a, b),
            unit: Unit::Word,
            head: b,
        }
    }

    /// A triple click: the whole paragraph at `at`.
    pub fn paragraph(paras: &[&str], at: Pos) -> Self {
        let len = text_of(paras, at.para).len();
        let (a, b) = (Pos::new(at.para, 0), Pos::new(at.para, len));
        Self {
            origin: (a, b),
            unit: Unit::Para,
            head: b,
        }
    }

    /// A drag or a shift-click: the moving end goes to `head`; what was picked
    /// first (the word, the paragraph) stays whole.
    pub fn extend_to(&mut self, head: Pos) {
        self.head = head;
    }

    /// The selected span, start before end, with the moving end snapped to the
    /// unit (whole words, whole paragraphs).
    pub fn span(&self, paras: &[&str]) -> (Pos, Pos) {
        let (o_start, o_end) = self.origin;
        let h = Pos::new(
            self.head.para,
            clamp(text_of(paras, self.head.para), self.head.offset),
        );
        let snap = |at: Pos, forward: bool| -> Pos {
            let text = text_of(paras, at.para);
            match self.unit {
                Unit::Char => at,
                Unit::Word => {
                    let r = word_range(text, at.offset);
                    Pos::new(at.para, if forward { r.end } else { r.start })
                }
                Unit::Para => Pos::new(at.para, if forward { text.len() } else { 0 }),
            }
        };
        if h < o_start {
            (snap(h, false), o_end)
        } else if h > o_end {
            (o_start, snap(h, true))
        } else {
            (o_start, o_end)
        }
    }

    /// Whether nothing is selected (a plain click, or a drag back to where it began).
    pub fn is_empty(&self, paras: &[&str]) -> bool {
        let (a, b) = self.span(paras);
        a == b
    }

    /// The part of paragraph `para` that is selected, for the highlight.
    pub fn range_in(&self, paras: &[&str], para: usize) -> Option<Range<usize>> {
        let (a, b) = self.span(paras);
        if para < a.para || para > b.para {
            return None;
        }
        let len = text_of(paras, para).len();
        let start = if para == a.para { a.offset.min(len) } else { 0 };
        let end = if para == b.para { b.offset.min(len) } else { len };
        (start < end).then_some(start..end)
    }

    /// The selected part of each paragraph it touches, with the paragraph's index:
    /// what [`Self::text`] joins (an empty paragraph in the middle is kept).
    pub fn slices(&self, paras: &[&str]) -> Vec<(usize, String)> {
        let (a, b) = self.span(paras);
        (a.para..=b.para)
            .map(|para| {
                let text = text_of(paras, para);
                let start = if para == a.para { clamp(text, a.offset) } else { 0 };
                let end = if para == b.para { clamp(text, b.offset) } else { text.len() };
                (para, text[start.min(end)..end].to_owned())
            })
            .collect()
    }

    /// The selected text, paragraphs apart by a line break.
    pub fn text(&self, paras: &[&str]) -> String {
        let (a, b) = self.span(paras);
        let mut out = Vec::new();
        for para in a.para..=b.para {
            let text = text_of(paras, para);
            let start = if para == a.para { clamp(text, a.offset) } else { 0 };
            let end = if para == b.para { clamp(text, b.offset) } else { text.len() };
            out.push(&text[start.min(end)..end]);
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAS: [&str; 3] = ["Hello brave world", "second one", "third, é paragraph"];

    #[test]
    fn a_double_click_takes_the_word_and_a_drag_keeps_whole_words() {
        let mut s = Selection::word(&PARAS, Pos::new(0, 8));
        assert_eq!(s.text(&PARAS), "brave");
        s.extend_to(Pos::new(0, 14));
        assert_eq!(s.text(&PARAS), "brave world");
        s.extend_to(Pos::new(0, 2));
        assert_eq!(s.text(&PARAS), "Hello brave");
    }

    #[test]
    fn words_stop_at_punctuation_and_spaces() {
        assert_eq!(&"third, é paragraph"[word_range("third, é paragraph", 2)], "third");
        assert_eq!(&"third, é paragraph"[word_range("third, é paragraph", 5)], ",");
        assert_eq!(&"third, é paragraph"[word_range("third, é paragraph", 8)], "é");
        assert_eq!(word_range("hi", 2), 0..2);
        assert_eq!(word_range("", 0), 0..0);
    }

    #[test]
    fn a_triple_click_takes_the_paragraph_and_a_drag_whole_paragraphs() {
        let mut s = Selection::paragraph(&PARAS, Pos::new(1, 3));
        assert_eq!(s.text(&PARAS), "second one");
        s.extend_to(Pos::new(2, 1));
        assert_eq!(s.text(&PARAS), "second one\nthird, é paragraph");
        s.extend_to(Pos::new(0, 4));
        assert_eq!(s.text(&PARAS), "Hello brave world\nsecond one");
    }

    #[test]
    fn a_drag_across_paragraphs_runs_from_the_anchor_to_the_pointer_either_way() {
        let mut s = Selection::caret(Pos::new(0, 6));
        assert!(s.is_empty(&PARAS));
        s.extend_to(Pos::new(2, 5));
        assert_eq!(s.text(&PARAS), "brave world\nsecond one\nthird");
        s.extend_to(Pos::new(0, 0));
        assert_eq!(s.text(&PARAS), "Hello ");
    }

    #[test]
    fn the_highlight_range_of_each_paragraph() {
        let mut s = Selection::caret(Pos::new(0, 6));
        s.extend_to(Pos::new(2, 5));
        assert_eq!(s.range_in(&PARAS, 0), Some(6..17));
        assert_eq!(s.range_in(&PARAS, 1), Some(0..10));
        assert_eq!(s.range_in(&PARAS, 2), Some(0..5));
        assert_eq!(s.range_in(&PARAS, 3), None);
        let none = Selection::caret(Pos::new(1, 2));
        assert_eq!(none.range_in(&PARAS, 1), None);
    }

    #[test]
    fn shift_click_extends_from_the_anchor_and_keeps_the_unit() {
        let mut s = Selection::word(&PARAS, Pos::new(0, 1));
        s.extend_to(Pos::new(1, 3));
        assert_eq!(s.text(&PARAS), "Hello brave world\nsecond");
    }

    #[test]
    fn the_slices_are_the_selected_part_of_each_paragraph() {
        let mut s = Selection::caret(Pos::new(0, 6));
        s.extend_to(Pos::new(2, 5));
        assert_eq!(
            s.slices(&PARAS),
            vec![(0, "brave world".to_owned()), (1, "second one".to_owned()), (2, "third".to_owned())]
        );
    }

    #[test]
    fn offsets_inside_a_character_move_to_its_start() {
        let mut s = Selection::caret(Pos::new(2, 0));
        s.extend_to(Pos::new(2, 8)); // inside the two bytes of é
        assert_eq!(s.text(&PARAS), "third, ");
    }
}
