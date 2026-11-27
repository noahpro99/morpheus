//! Message blocks (`rich`) flattened to lines for a screen that draws text in one run of paragraphs, like a phone:
//! each line carries how deep it is quoted and listed, the marker that leads a list item, and its styled runs.

use crate::rich::{Block, Para, Style};

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Para,
    /// A line of a code block.
    Code,
    Rule,
}

/// A piece of a line's text with one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// How many block quotes it is in.
    pub quote: u32,
    /// How many lists it is in.
    pub indent: u32,
    /// "•" or "3." on the first line of a list item.
    pub marker: Option<String>,
    pub runs: Vec<Run>,
}

/// The runs of a paragraph: its text cut where the style changes.
pub fn runs(para: &Para) -> Vec<Run> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut spans: Vec<_> = para.spans.iter().collect();
    spans.sort_by_key(|s| s.range.start);
    for span in spans {
        let (start, end) = (span.range.start.max(at), span.range.end.min(para.text.len()));
        if start >= end || !para.text.is_char_boundary(start) || !para.text.is_char_boundary(end) {
            continue;
        }
        if start > at {
            out.push(Run { text: para.text[at..start].to_owned(), style: Style::default() });
        }
        out.push(Run { text: para.text[start..end].to_owned(), style: span.style.clone() });
        at = end;
    }
    if at < para.text.len() {
        out.push(Run { text: para.text[at..].to_owned(), style: Style::default() });
    }
    out
}

pub fn lines(blocks: &[Block]) -> Vec<Line> {
    let mut out = Vec::new();
    walk(blocks, 0, 0, &mut out);
    out
}

fn walk(blocks: &[Block], quote: u32, indent: u32, out: &mut Vec<Line>) {
    for block in blocks {
        match block {
            Block::Para(para) => out.push(Line { kind: LineKind::Para, quote, indent, marker: None, runs: runs(para) }),
            Block::Pre(text) => {
                for line in text.trim_end_matches('\n').split('\n') {
                    out.push(Line {
                        kind: LineKind::Code,
                        quote,
                        indent,
                        marker: None,
                        runs: vec![Run { text: line.to_owned(), style: Style { code: true, ..Style::default() } }],
                    });
                }
            }
            Block::Quote(inner) => walk(inner, quote + 1, indent, out),
            Block::List { ordered, start, items } => {
                for (n, item) in items.iter().enumerate() {
                    let first = out.len();
                    walk(item, quote, indent + 1, out);
                    let marker = if *ordered { format!("{}.", start + n as u32) } else { "\u{2022}".to_owned() };
                    match out[first..].iter_mut().find(|l| l.indent == indent + 1 && l.marker.is_none()) {
                        Some(line) => line.marker = Some(marker),
                        None => out.push(Line { kind: LineKind::Para, quote, indent: indent + 1, marker: Some(marker), runs: Vec::new() }),
                    }
                }
            }
            Block::Rule => out.push(Line { kind: LineKind::Rule, quote, indent, marker: None, runs: Vec::new() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich;

    fn text(line: &Line) -> String {
        line.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn bold_and_code_runs_split_the_text() {
        let lines = lines(&rich::html("<p>a <b>bold</b> and <code>code</code>!</p>"));
        assert_eq!(lines.len(), 1);
        let runs = &lines[0].runs;
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["a ", "bold", " and ", "code", "!"]);
        assert!(runs[1].style.bold && !runs[0].style.bold);
        assert!(runs[3].style.code);
    }

    #[test]
    fn quotes_lists_and_code_blocks_become_lines() {
        let lines = lines(&rich::html("<blockquote><p>quoted</p></blockquote><ul><li>one</li><li>two</li></ul><ol start=\"3\"><li>c</li></ol><pre><code>x = 1\ny</code></pre><hr>"));
        assert_eq!((lines[0].quote, text(&lines[0])), (1, "quoted".to_owned()));
        assert_eq!((lines[1].indent, lines[1].marker.as_deref(), text(&lines[1])), (1, Some("\u{2022}"), "one".to_owned()));
        assert_eq!(lines[2].marker.as_deref(), Some("\u{2022}"));
        assert_eq!(lines[3].marker.as_deref(), Some("3."));
        assert_eq!((lines[4].kind, text(&lines[4])), (LineKind::Code, "x = 1".to_owned()));
        assert_eq!(text(&lines[5]), "y");
        assert_eq!(lines[6].kind, LineKind::Rule);
    }

    #[test]
    fn an_emoji_run_keeps_its_picture() {
        let para = {
            let mut p = Para::default();
            p.push("hi ", Style::default());
            p.push(":party:", Style { emote: Some("mxc://s/x".into()), ..Style::default() });
            p
        };
        let runs = runs(&para);
        assert_eq!(runs[1].style.emote.as_deref(), Some("mxc://s/x"));
    }
}
