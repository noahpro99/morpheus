//! Message text as styled paragraphs, with no GPUI in it: the sanitised HTML
//! the core hands over (`MessageKind::Text { html }`, already stripped to the
//! Matrix subset) or a plain body with its bare URLs found, as blocks of text
//! plus style spans. `message.rs` turns a paragraph into GPUI text runs.
//!
//! The subset, as `crates/ui/assets/theme.css` (`.rich`) styles it: bold,
//! italic, underline, strike-through, inline code, links (mentions are links
//! to matrix.to), paragraphs, line breaks, headings, block quotes, lists,
//! code blocks and rules; tables read as lines of cells.

use std::{ops::Range, sync::Arc};

use ruma_html::{Html, NodeData, NodeRef};

/// Whether a link is a pill: a matrix.to link to a person or a room (not to
/// one message of it).
pub fn is_pill(href: &str) -> bool {
    let Some(rest) = href.strip_prefix("https://matrix.to/#/") else {
        return false;
    };
    let person_or_room = ["@", "%40", "#", "%23", "!", "%21"].iter().any(|p| rest.starts_with(p));
    person_or_room && !rest.contains("/$") && !rest.contains("/%24")
}

/// What a run of text looks like.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    /// Quiet text (the "(edited)" tag).
    pub faint: bool,
    /// A link to a person or a room (matrix.to): a mention, drawn as a pill.
    pub mention: bool,
    /// Where a click goes.
    pub link: Option<Arc<str>>,
}

/// A byte range of a paragraph's text and its style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
}

/// One paragraph: its text (line breaks inside are `\n`) and the styled ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Para {
    pub text: String,
    pub spans: Vec<Span>,
}

impl Para {
    /// The links in it: byte range and target.
    pub fn links(&self) -> Vec<(Range<usize>, Arc<str>)> {
        self.spans
            .iter()
            .filter_map(|s| s.style.link.clone().map(|l| (s.range.clone(), l)))
            .collect()
    }

    /// Adds text with `style` at the end.
    pub fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        self.add_span(start..self.text.len(), style);
    }

    fn add_span(&mut self, range: Range<usize>, style: Style) {
        if style == Style::default() {
            return;
        }
        // Text nodes of one element come as several pushes: one span.
        if let Some(last) = self.spans.last_mut()
            && last.style == style
            && last.range.end == range.start
        {
            last.range.end = range.end;
            return;
        }
        self.spans.push(Span { range, style });
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Para(Para),
    /// A code block, as typed.
    Pre(String),
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
    Rule,
}

/// What a test sees as the text of blocks (as the DOM's `textContent`: no
/// separators between blocks, no list markers).
pub fn text_of(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Para(p) => out.push_str(&p.text),
            Block::Pre(text) => out.push_str(text),
            Block::Quote(inner) => out.push_str(&text_of(inner)),
            Block::List { items, .. } => {
                for item in items {
                    out.push_str(&text_of(item));
                }
            }
            Block::Rule => {}
        }
    }
    out
}

/// A plain body: its text as written (line breaks kept), bare web links found.
pub fn plain(body: &str) -> Vec<Block> {
    let mut para = Para::default();
    for segment in super::linkify::segments(body) {
        match segment {
            super::linkify::Segment::Text(text) => para.push(&text, Style::default()),
            super::linkify::Segment::Link { text, url } => para.push(
                &text,
                Style {
                    link: Some(url.into()),
                    ..Style::default()
                },
            ),
        }
    }
    vec![Block::Para(para)]
}

/// Sanitised HTML as blocks.
pub fn html(source: &str) -> Vec<Block> {
    let parsed = Html::parse(source);
    let mut out = Vec::new();
    let mut inline = Inline::default();
    for node in parsed.children() {
        walk(&node, &Style::default(), &mut inline, &mut out);
    }
    flush(&mut inline, &mut out);
    out
}

/// Text being gathered into a paragraph, with HTML's whitespace rules (runs
/// of white space are one space; none at the start or end of a line).
#[derive(Default)]
struct Inline {
    para: Para,
}

impl Inline {
    fn text(&mut self, text: &str, style: &Style) {
        let mut piece = String::new();
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                let after_break = if piece.is_empty() {
                    self.para.text.is_empty() || self.para.text.ends_with([' ', '\n'])
                } else {
                    piece.ends_with(' ')
                };
                if !after_break {
                    piece.push(' ');
                }
            } else {
                piece.push(c);
            }
        }
        self.para.push(&piece, style.clone());
    }

    fn line_break(&mut self) {
        self.trim_end();
        self.para.text.push('\n');
    }

    fn trim_end(&mut self) {
        let trimmed = self.para.text.trim_end_matches(' ').len();
        self.para.text.truncate(trimmed);
        for span in &mut self.para.spans {
            span.range.end = span.range.end.min(trimmed);
        }
        self.para.spans.retain(|s| s.range.start < s.range.end);
    }
}

fn flush(inline: &mut Inline, out: &mut Vec<Block>) {
    inline.trim_end();
    let mut para = std::mem::take(&mut inline.para);
    // A trailing break closes the line, it does not open an empty one.
    while para.text.ends_with('\n') {
        para.text.pop();
        let len = para.text.len();
        for span in &mut para.spans {
            span.range.end = span.range.end.min(len);
        }
        para.spans.retain(|s| s.range.start < s.range.end);
    }
    if !para.text.is_empty() {
        out.push(Block::Para(para));
    }
}

fn attr(node: &NodeRef, name: &str) -> Option<String> {
    let el = node.as_element()?;
    el.attrs
        .borrow()
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| a.value.to_string())
}

fn text_content(node: &NodeRef, out: &mut String) {
    match node.data() {
        NodeData::Text(t) => out.push_str(&t.borrow()),
        NodeData::Element(el) if &*el.name.local == "br" => out.push('\n'),
        _ => {
            for child in node.children() {
                text_content(&child, out);
            }
        }
    }
}

fn sub_blocks(node: &NodeRef, style: &Style) -> Vec<Block> {
    let mut out = Vec::new();
    let mut inline = Inline::default();
    for child in node.children() {
        walk(&child, style, &mut inline, &mut out);
    }
    flush(&mut inline, &mut out);
    out
}

fn walk(node: &NodeRef, style: &Style, inline: &mut Inline, out: &mut Vec<Block>) {
    let el = match node.data() {
        NodeData::Text(text) => {
            // Bare web links in the text of a formatted message are links too
            // (but not inside a link or code).
            if style.link.is_some() || style.code {
                inline.text(&text.borrow(), style);
            } else {
                for segment in super::linkify::segments(&text.borrow()) {
                    match segment {
                        super::linkify::Segment::Text(piece) => inline.text(&piece, style),
                        super::linkify::Segment::Link { text, url } => {
                            let mut linked = style.clone();
                            linked.link = Some(url.into());
                            inline.para.push(&text, linked);
                        }
                    }
                }
            }
            return;
        }
        NodeData::Element(el) => el,
        _ => return,
    };
    let name: &str = &el.name.local;
    let styled = |apply: &dyn Fn(&mut Style)| {
        let mut next = style.clone();
        apply(&mut next);
        next
    };
    match name {
        "br" => inline.line_break(),
        "p" | "div" | "section" | "center" | "details" | "summary" | "li" | "table" | "thead"
        | "tbody" | "tfoot" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            flush(inline, out);
            let inner = if name.starts_with('h') && name.len() == 2 {
                styled(&|s| s.bold = true)
            } else {
                style.clone()
            };
            out.extend(sub_blocks(node, &inner));
        }
        "blockquote" => {
            flush(inline, out);
            out.push(Block::Quote(sub_blocks(node, style)));
        }
        "ul" | "ol" => {
            flush(inline, out);
            let items = node
                .children()
                .filter(|c| c.as_element().is_some_and(|e| &*e.name.local == "li"))
                .map(|li| sub_blocks(&li, style))
                .collect();
            out.push(Block::List {
                ordered: name == "ol",
                start: attr(node, "start")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1),
                items,
            });
        }
        "pre" => {
            flush(inline, out);
            let mut code = String::new();
            text_content(node, &mut code);
            out.push(Block::Pre(code.trim_end_matches('\n').to_owned()));
        }
        "hr" => {
            flush(inline, out);
            out.push(Block::Rule);
        }
        "strong" | "b" => walk_children(node, &styled(&|s| s.bold = true), inline, out),
        "em" | "i" => walk_children(node, &styled(&|s| s.italic = true), inline, out),
        "u" => walk_children(node, &styled(&|s| s.underline = true), inline, out),
        "s" | "strike" | "del" => walk_children(node, &styled(&|s| s.strike = true), inline, out),
        "code" => walk_children(node, &styled(&|s| s.code = true), inline, out),
        "a" => match attr(node, "href") {
            Some(href) => {
                let mention = is_pill(&href);
                let next = styled(&|s| {
                    s.link = Some(href.as_str().into());
                    s.mention = mention;
                });
                walk_children(node, &next, inline, out);
            }
            None => walk_children(node, style, inline, out),
        },
        "img" => {
            let alt = attr(node, "alt")
                .or_else(|| attr(node, "title"))
                .unwrap_or_else(|| "image".into());
            inline.text(&format!("[{alt}]"), style);
        }
        "td" | "th" => {
            walk_children(node, style, inline, out);
            inline.text(" ", style);
        }
        _ => walk_children(node, style, inline, out),
    }
}

fn walk_children(node: &NodeRef, style: &Style, inline: &mut Inline, out: &mut Vec<Block>) {
    for child in node.children() {
        walk(&child, style, inline, out);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn para(blocks: &[Block], at: usize) -> &Para {
        match &blocks[at] {
            Block::Para(p) => p,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    fn styled(p: &Para, needle: &str) -> Style {
        let start = p.text.find(needle).expect("text present");
        p.spans
            .iter()
            .find(|s| s.range.start <= start && start < s.range.end)
            .map(|s| s.style.clone())
            .unwrap_or_default()
    }

    #[test]
    fn inline_styles_become_spans() {
        let blocks = html("<p>a <strong>bold</strong> and <em>it</em> <del>x</del> <code>c</code></p>");
        assert_eq!(blocks.len(), 1);
        let p = para(&blocks, 0);
        assert_eq!(p.text, "a bold and it x c");
        assert!(styled(p, "bold").bold);
        assert!(styled(p, "it").italic);
        assert!(styled(p, "x").strike);
        assert!(styled(p, "c").code);
        assert_eq!(styled(p, "a "), Style::default());
    }

    #[test]
    fn nested_styles_combine() {
        let blocks = html("<b><i>both</i> bold</b>");
        let p = para(&blocks, 0);
        let both = styled(p, "both");
        assert!(both.bold && both.italic);
        let bold = styled(p, "bold");
        assert!(bold.bold && !bold.italic);
    }

    #[test]
    fn links_and_mentions_keep_their_targets() {
        let blocks = html(
            "hi <a href=\"https://matrix.to/#/@bob:localhost\">Bob</a> see <a href=\"https://example.org\">here</a>",
        );
        let p = para(&blocks, 0);
        assert_eq!(p.text, "hi Bob see here");
        let links = p.links();
        assert_eq!(links.len(), 2);
        assert_eq!(&p.text[links[0].0.clone()], "Bob");
        assert_eq!(&*links[0].1, "https://matrix.to/#/@bob:localhost");
        assert!(p.spans[0].style.mention && !p.spans[1].style.mention);
        assert_eq!(&p.text[links[1].0.clone()], "here");
        assert_eq!(&*links[1].1, "https://example.org");
    }

    #[test]
    fn bare_urls_in_formatted_text_are_links_except_in_code() {
        let blocks = html(
            "<p><strong>bold</strong> and https://example.org/rich-1. <code>https://example.org/code</code> <a href=\"https://a.example\">https://b.example</a></p>",
        );
        let p = para(&blocks, 0);
        let links = p.links();
        assert_eq!(links.len(), 2, "{links:?}");
        assert_eq!(&p.text[links[0].0.clone()], "https://example.org/rich-1");
        assert_eq!(&*links[0].1, "https://example.org/rich-1");
        assert_eq!(&*links[1].1, "https://a.example", "an anchor keeps its own target");
        assert!(p.text.contains("rich-1. https://example.org/code"));
    }

    #[test]
    fn an_anchor_without_a_target_is_plain_text() {
        let p = &html("<a>nowhere</a>");
        assert!(para(p, 0).links().is_empty());
    }

    #[test]
    fn white_space_collapses_and_breaks_stay() {
        let blocks = html("<p>one \n  two<br>three<br/></p>");
        assert_eq!(para(&blocks, 0).text, "one two\nthree");
    }

    #[test]
    fn paragraphs_quotes_lists_and_code_blocks() {
        let blocks = html(
            "<p>first</p><blockquote><p>quoted</p></blockquote><ul><li>a</li><li>b</li></ul>\
             <ol start=\"3\"><li>c</li></ol><pre><code>let x = 1;\n  y\n</code></pre><hr>",
        );
        assert_eq!(blocks.len(), 6);
        assert_eq!(para(&blocks, 0).text, "first");
        match &blocks[1] {
            Block::Quote(inner) => assert_eq!(para(inner, 0).text, "quoted"),
            other => panic!("{other:?}"),
        }
        match &blocks[2] {
            Block::List {
                ordered,
                start,
                items,
            } => {
                assert!(!ordered);
                assert_eq!(*start, 1);
                assert_eq!(items.len(), 2);
                assert_eq!(text_of(&items[1]), "b");
            }
            other => panic!("{other:?}"),
        }
        match &blocks[3] {
            Block::List { ordered, start, .. } => assert!(*ordered && *start == 3),
            other => panic!("{other:?}"),
        }
        assert_eq!(blocks[4], Block::Pre("let x = 1;\n  y".into()));
        assert_eq!(blocks[5], Block::Rule);
        assert_eq!(text_of(&blocks), "firstquotedabclet x = 1;\n  y");
    }

    #[test]
    fn headings_are_bold() {
        let blocks = html("<h2>Title</h2><p>text</p>");
        assert!(styled(para(&blocks, 0), "Title").bold);
        assert!(!styled(para(&blocks, 1), "text").bold);
    }

    #[test]
    fn plain_bodies_find_links_and_keep_line_breaks() {
        let blocks = plain("see https://example.org/x.\nsecond line");
        let p = para(&blocks, 0);
        assert_eq!(p.text, "see https://example.org/x.\nsecond line");
        let links = p.links();
        assert_eq!(links.len(), 1);
        assert_eq!(&p.text[links[0].0.clone()], "https://example.org/x");
        assert_eq!(&*links[0].1, "https://example.org/x");
    }

    #[test]
    fn images_read_as_their_description() {
        let blocks = html("<img src=\"mxc://x/y\" alt=\"a cat\">");
        assert_eq!(para(&blocks, 0).text, "[a cat]");
    }
}

#[cfg(test)]
mod pill_tests {
    use super::is_pill;

    #[test]
    fn people_and_rooms_are_pills_but_messages_and_sites_are_not() {
        assert!(is_pill("https://matrix.to/#/@bob:localhost"));
        assert!(is_pill("https://matrix.to/#/%40bob:localhost"));
        assert!(is_pill("https://matrix.to/#/#random:localhost"));
        assert!(is_pill("https://matrix.to/#/!abc:localhost?via=localhost"));
        assert!(!is_pill("https://matrix.to/#/!abc:localhost/$event"));
        assert!(!is_pill("https://example.org/#/@bob:localhost"));
    }
}
