//! Finds web links in plain message text, so they can be rendered as links.

/// A run of message text: plain, or a link with the URL to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Link { text: String, url: String },
}

/// Split `text` into plain runs and links (`http://`, `https://`, `www.`).
/// A link runs to the next whitespace, minus trailing punctuation that
/// belongs to the sentence (a full stop, an unmatched closing bracket).
pub fn segments(text: &str) -> Vec<Segment> {
    const PREFIXES: [&str; 3] = ["https://", "http://", "www."];
    let mut out = Vec::new();
    let mut plain_start = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let word_start = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '/' && c != '.');
        let prefix = PREFIXES.iter().find(|p| {
            rest.get(..p.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(p))
        });
        if let Some(prefix) = prefix
            && word_start
        {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let link = trim_trailing(&rest[..end]);
            if link.len() > prefix.len() {
                if plain_start < i {
                    out.push(Segment::Text(text[plain_start..i].to_owned()));
                }
                let url = if prefix.starts_with("www") {
                    format!("https://{link}")
                } else {
                    link.to_owned()
                };
                out.push(Segment::Link {
                    text: link.to_owned(),
                    url,
                });
                i += link.len();
                plain_start = i;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    if plain_start < text.len() {
        out.push(Segment::Text(text[plain_start..].to_owned()));
    }
    out
}

/// Drop sentence punctuation from the end of a link; a closing bracket stays
/// only when the link opened one (`.../Rust_(language)`).
fn trim_trailing(mut link: &str) -> &str {
    loop {
        let Some(last) = link.chars().next_back() else {
            return link;
        };
        let unmatched =
            |open: char, close: char| link.matches(close).count() > link.matches(open).count();
        let drop = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '"' => true,
            ')' => unmatched('(', ')'),
            ']' => unmatched('[', ']'),
            '}' => unmatched('{', '}'),
            '>' => true,
            _ => false,
        };
        if !drop {
            return link;
        }
        link = &link[..link.len() - last.len_utf8()];
    }
}

#[cfg(test)]
mod tests {
    use super::{Segment, segments};

    fn link(text: &str, url: &str) -> Segment {
        Segment::Link {
            text: text.into(),
            url: url.into(),
        }
    }

    fn text(t: &str) -> Segment {
        Segment::Text(t.into())
    }

    #[test]
    fn plain_text_stays_plain() {
        assert_eq!(segments("hello there"), [text("hello there")]);
        assert_eq!(segments(""), Vec::<Segment>::new());
    }

    #[test]
    fn urls_become_links() {
        assert_eq!(
            segments("see https://example.org/a?b=c#d now"),
            [
                text("see "),
                link("https://example.org/a?b=c#d", "https://example.org/a?b=c#d"),
                text(" now")
            ]
        );
        assert_eq!(segments("http://x.y"), [link("http://x.y", "http://x.y")]);
    }

    #[test]
    fn www_gets_a_scheme() {
        assert_eq!(
            segments("go to www.matrix.org."),
            [
                text("go to "),
                link("www.matrix.org", "https://www.matrix.org"),
                text(".")
            ]
        );
    }

    #[test]
    fn trailing_punctuation_is_not_part_of_the_link() {
        assert_eq!(
            segments("(https://a.b/c), and https://d.e/f!"),
            [
                text("("),
                link("https://a.b/c", "https://a.b/c"),
                text("), and "),
                link("https://d.e/f", "https://d.e/f"),
                text("!")
            ]
        );
        // Balanced brackets inside a URL stay (Wikipedia style).
        assert_eq!(
            segments("https://en.wikipedia.org/wiki/Rust_(language)"),
            [link(
                "https://en.wikipedia.org/wiki/Rust_(language)",
                "https://en.wikipedia.org/wiki/Rust_(language)"
            )]
        );
    }

    #[test]
    fn a_bare_scheme_is_not_a_link() {
        assert_eq!(segments("https:// nothing"), [text("https:// nothing")]);
    }
}
