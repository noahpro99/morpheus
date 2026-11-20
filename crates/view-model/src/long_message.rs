//! Very long messages are drawn collapsed (#104): their first lines with a
//! fade and a "Show more" bar that opens the whole text full screen. Whether
//! a message is long is decided here, from its text alone and never from the
//! window's width, so a row's collapsed height is the same on every redraw
//! and the timeline's scroll position and measured heights stay stable.

use crate::rich::Block;

/// A message of more than this many lines is collapsed.
pub const COLLAPSE_LINES: usize = 20;
/// Lines of a collapsed message that stay in view.
pub const SHOWN_LINES: usize = 12;
/// Characters to a line, for lines that wrap (a fixed estimate, not the
/// window's measure).
pub const WRAP_COLS: usize = 100;

/// How a long message is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Collapse {
    /// The message's lines, as estimated.
    pub lines: usize,
    /// How many of them stay in view.
    pub shown: usize,
}

/// Lines `text` takes: each line break starts one, a long line wraps.
fn text_lines(text: &str) -> usize {
    text.split('\n')
        .map(|line| line.chars().count().div_ceil(WRAP_COLS).max(1))
        .sum()
}

/// The lines the blocks take, as estimated.
pub fn estimated_lines(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::Para(para) => text_lines(&para.text),
            Block::Pre(code) => text_lines(code.trim_end_matches('\n')),
            Block::Quote(inner) => estimated_lines(inner),
            Block::List { items, .. } => items.iter().map(|item| estimated_lines(item)).sum(),
            Block::Rule => 1,
        })
        .sum()
}

/// `Some` when a message made of `blocks` is drawn collapsed.
pub fn collapse(blocks: &[Block]) -> Option<Collapse> {
    let lines = estimated_lines(blocks);
    (lines > COLLAPSE_LINES).then_some(Collapse {
        lines,
        shown: SHOWN_LINES,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich;

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_short_message_is_not_collapsed() {
        assert_eq!(collapse(&rich::plain("hello")), None);
        assert_eq!(collapse(&rich::plain(&lines(COLLAPSE_LINES))), None);
    }

    #[test]
    fn a_message_of_many_lines_is_collapsed() {
        let c = collapse(&rich::plain(&lines(COLLAPSE_LINES + 1))).expect("collapsed");
        assert_eq!(c.lines, COLLAPSE_LINES + 1);
        assert!(c.shown < COLLAPSE_LINES);
    }

    #[test]
    fn a_long_line_counts_as_the_lines_it_wraps_to() {
        let wall = "x".repeat(WRAP_COLS * (COLLAPSE_LINES + 1));
        assert!(collapse(&rich::plain(&wall)).is_some());
        let one = "x".repeat(WRAP_COLS);
        assert_eq!(estimated_lines(&rich::plain(&one)), 1);
    }

    #[test]
    fn code_blocks_and_lists_count_their_lines() {
        let code = vec![Block::Pre(lines(30))];
        assert_eq!(estimated_lines(&code), 30);
        assert!(collapse(&code).is_some());
        let list = vec![Block::List {
            ordered: false,
            start: 1,
            items: (0..25).map(|_| rich::plain("item")).collect(),
        }];
        assert_eq!(estimated_lines(&list), 25);
        assert!(collapse(&list).is_some());
    }

    #[test]
    fn the_decision_depends_on_the_text_only() {
        let blocks = rich::plain(&lines(40));
        assert_eq!(collapse(&blocks), collapse(&blocks));
    }
}
