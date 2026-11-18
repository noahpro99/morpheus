//! Patch notes: a GitHub release's markdown body as the app's rich blocks,
//! and which version's notes this device has been shown (a "What's new"
//! popup once after an update; `whats_new.json` in the data directory).

use std::path::Path;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::rich::{Block, Para, Style};

const FILE: &str = "whats_new.json";

/// Shown in Help when the notes could not be loaded.
pub const UNAVAILABLE: &str = "Couldn't load the patch notes.";

/// The release's page on GitHub.
pub fn release_page(version: &str) -> String {
    format!("https://github.com/noahpro99/morpheus/releases/tag/v{version}")
}

/// The popup's title.
pub fn popup_title(version: &str) -> String {
    format!("What's new in {version}")
}

/// The last version whose notes were shown on this device.
pub fn seen(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(value.get("version")?.as_str()?.to_owned())
}

pub fn mark_seen(dir: &Path, version: &str) -> std::io::Result<()> {
    std::fs::write(dir.join(FILE), serde_json::json!({ "version": version }).to_string())
}

/// Whether the popup is due: the app was used before (a version is on
/// record) and this is a different one. A first install shows nothing.
pub fn popup_due(seen: Option<&str>, running: &str) -> bool {
    seen.is_some_and(|seen| seen != running)
}

enum Frame {
    Quote(Vec<Block>),
    List { ordered: bool, start: u32, items: Vec<Vec<Block>> },
    Item(Vec<Block>),
}

struct Builder {
    root: Vec<Block>,
    frames: Vec<Frame>,
    para: Option<Para>,
    code: Option<String>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: Option<std::sync::Arc<str>>,
}

impl Builder {
    fn blocks(&mut self) -> &mut Vec<Block> {
        match self.frames.last_mut() {
            Some(Frame::Quote(blocks) | Frame::Item(blocks)) => blocks,
            _ => &mut self.root,
        }
    }

    fn flush(&mut self) {
        if let Some(para) = self.para.take()
            && !para.text.is_empty()
        {
            self.blocks().push(Block::Para(para));
        }
    }

    fn style(&self, code: bool) -> Style {
        Style {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code,
            link: self.link.clone(),
            underline: self.link.is_some(),
            ..Style::default()
        }
    }

    fn text(&mut self, text: &str, code: bool) {
        if let Some(code_block) = &mut self.code {
            code_block.push_str(text);
            return;
        }
        let style = self.style(code);
        self.para.get_or_insert_with(Para::default).push(text, style);
    }

    fn close_container(&mut self, ending: TagEnd) {
        self.flush();
        let Some(frame) = self.frames.pop() else { return };
        let block = match (frame, ending) {
            (Frame::Quote(inner), TagEnd::BlockQuote(_)) => Block::Quote(inner),
            (Frame::List { ordered, start, items }, TagEnd::List(_)) => {
                Block::List { ordered, start, items }
            }
            (Frame::Item(inner), TagEnd::Item) => {
                if let Some(Frame::List { items, .. }) = self.frames.last_mut() {
                    items.push(inner);
                }
                return;
            }
            _ => return,
        };
        self.blocks().push(block);
    }
}

/// A markdown document as blocks: paragraphs, headings (bold paragraphs),
/// lists, quotes, code blocks, rules; bold, italic, strike, code and links.
pub fn blocks(markdown: &str) -> Vec<Block> {
    let mut b = Builder {
        root: Vec::new(),
        frames: Vec::new(),
        para: None,
        code: None,
        bold: 0,
        italic: 0,
        strike: 0,
        link: None,
    };
    for event in Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH) {
        match event {
            Event::Start(Tag::Paragraph) => b.flush(),
            Event::End(TagEnd::Paragraph) => b.flush(),
            Event::Start(Tag::Heading { .. }) => {
                b.flush();
                b.bold += 1;
            }
            Event::End(TagEnd::Heading(_)) => {
                b.flush();
                b.bold -= 1;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                b.flush();
                b.frames.push(Frame::Quote(Vec::new()));
            }
            Event::Start(Tag::List(first)) => {
                b.flush();
                b.frames.push(Frame::List {
                    ordered: first.is_some(),
                    start: first.map_or(1, |n| n as u32),
                    items: Vec::new(),
                });
            }
            Event::Start(Tag::Item) => b.frames.push(Frame::Item(Vec::new())),
            Event::End(end @ (TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item)) => {
                b.close_container(end)
            }
            Event::Start(Tag::CodeBlock(_)) => {
                b.flush();
                b.code = Some(String::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(code) = b.code.take() {
                    b.blocks().push(Block::Pre(code.trim_end_matches('\n').to_owned()));
                }
            }
            Event::Start(Tag::Strong) => b.bold += 1,
            Event::End(TagEnd::Strong) => b.bold -= 1,
            Event::Start(Tag::Emphasis) => b.italic += 1,
            Event::End(TagEnd::Emphasis) => b.italic -= 1,
            Event::Start(Tag::Strikethrough) => b.strike += 1,
            Event::End(TagEnd::Strikethrough) => b.strike -= 1,
            Event::Start(Tag::Link { dest_url, .. }) => b.link = Some(dest_url.as_ref().into()),
            Event::End(TagEnd::Link) => b.link = None,
            Event::Text(text) => b.text(&text, false),
            Event::Code(text) => b.text(&text, true),
            Event::SoftBreak => b.text(" ", false),
            Event::HardBreak => b.text("\n", false),
            Event::Rule => {
                b.flush();
                b.blocks().push(Block::Rule);
            }
            _ => {}
        }
    }
    b.flush();
    b.root
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn para(blocks: &[Block], at: usize) -> &Para {
        match &blocks[at] {
            Block::Para(p) => p,
            other => panic!("not a paragraph: {other:?}"),
        }
    }

    #[test]
    fn headings_are_bold_paragraphs_and_text_is_styled() {
        let blocks = blocks("## Fixes\n\nA **bold** and *slanted* and `code` word.");
        assert_eq!(blocks.len(), 2);
        let heading = para(&blocks, 0);
        assert_eq!(heading.text, "Fixes");
        assert!(heading.spans.iter().all(|s| s.style.bold));
        let body = para(&blocks, 1);
        assert_eq!(body.text, "A bold and slanted and code word.");
        assert!(body.spans.iter().any(|s| s.style.bold && &body.text[s.range.clone()] == "bold"));
        assert!(body.spans.iter().any(|s| s.style.italic && &body.text[s.range.clone()] == "slanted"));
        assert!(body.spans.iter().any(|s| s.style.code && &body.text[s.range.clone()] == "code"));
    }

    #[test]
    fn lists_keep_their_items_and_order() {
        let blocks = blocks("- one\n- two with [a link](https://example.com/x)\n\n1. first\n2. second");
        let Block::List { ordered, items, .. } = &blocks[0] else { panic!("{blocks:?}") };
        assert!(!ordered);
        assert_eq!(items.len(), 2);
        assert_eq!(para(&items[0], 0).text, "one");
        let second = para(&items[1], 0);
        assert_eq!(second.text, "two with a link");
        assert_eq!(second.links().len(), 1);
        assert_eq!(&*second.links()[0].1, "https://example.com/x");
        let Block::List { ordered, items, .. } = &blocks[1] else { panic!("{blocks:?}") };
        assert!(ordered);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn code_blocks_quotes_and_rules() {
        let blocks = blocks("> quoted\n\n```\nlet x = 1;\n```\n\n---\n");
        assert!(matches!(&blocks[0], Block::Quote(inner) if para(inner, 0).text == "quoted"));
        assert_eq!(blocks[1], Block::Pre("let x = 1;".into()));
        assert_eq!(blocks[2], Block::Rule);
    }

    #[test]
    fn empty_notes_are_no_blocks() {
        assert!(blocks("").is_empty());
        assert!(blocks("  \n").is_empty());
    }

    #[test]
    fn the_popup_is_due_only_after_a_version_change() {
        assert!(!popup_due(None, "0.2.7"), "a first install shows nothing");
        assert!(!popup_due(Some("0.2.7"), "0.2.7"));
        assert!(popup_due(Some("0.2.6"), "0.2.7"));
    }

    #[test]
    fn the_seen_version_is_kept_per_device() {
        let dir = std::env::temp_dir().join(format!("morpheus-notes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(seen(&dir), None);
        mark_seen(&dir, "0.2.7").expect("save");
        assert_eq!(seen(&dir).as_deref(), Some("0.2.7"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn titles_and_pages_name_the_version() {
        assert_eq!(popup_title("0.2.7"), "What's new in 0.2.7");
        assert_eq!(
            release_page("0.2.7"),
            "https://github.com/noahpro99/morpheus/releases/tag/v0.2.7"
        );
    }
}
